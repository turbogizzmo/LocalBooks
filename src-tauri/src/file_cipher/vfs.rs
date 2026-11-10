//! The "offset" file system that lets an encrypted company file carry its key header (gap R1,
//! phase 2 of `docs/plans/encryption-at-rest.md`).
//!
//! An encrypted company file is `[key header][SQLCipher database]`. SQLite expects its database
//! at byte 0, so this VFS wraps the platform's default one and, for a main database file that
//! starts with [`SIGNATURE`], shifts every read, write, truncate and size by [`HEADER_LEN`]. It does
//! no cryptography: SQLCipher encrypts the pages, this only moves them. Every other file (the WAL,
//! the rollback journal, temporary files) and every main database without the signature (plain
//! company files, backups, imports) passes through unchanged, so all connections can use it.
//!
//! **Lock bytes.** SQLite locks byte ranges just past 1 GiB of the real file (`PENDING_BYTE`) and
//! never stores data in the page that holds them. Shifted by the header, a database page would land
//! on those bytes once the database passes 1 GiB, and Windows' mandatory locks would refuse writes
//! to it. So the mapping also skips [`LOCK_SPAN`] bytes at [`LOCK_START`] of the real file: database
//! offsets below `LOCK_START - HEADER_LEN` sit at `+HEADER_LEN`, the rest at
//! `+HEADER_LEN + LOCK_SPAN`. Both lengths are multiples of the largest page size, so no page read
//! or write crosses the boundary.
//!
//! The VFS registers once, as [`VFS_NAME`], and connections pick it with
//! `SqliteConnectOptions::vfs`.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::OnceLock;

use libsqlite3_sys as ffi;

/// The name connections ask for.
pub const VFS_NAME: &str = "localbooks";
const VFS_NAME_C: &CStr = c"localbooks";

/// The first bytes of every encrypted company file; never rewritten. Distinct from SQLite's
/// "SQLite format 3\0", so other programs see "not a database".
pub const SIGNATURE: &[u8; 16] = b"LocalBooks file\x01";
/// The key header's length: room for two copies of the slots, a multiple of the largest page.
pub const HEADER_LEN: i64 = 64 * 1024;
/// SQLite's lock bytes start at 1 GiB of the real file (`PENDING_BYTE`).
pub const LOCK_START: i64 = 0x4000_0000;
/// The bytes skipped there: one largest page.
pub const LOCK_SPAN: i64 = 64 * 1024;

/// Where database offset `offset` sits in a file with a header.
pub fn real_offset(offset: i64) -> i64 {
    if offset < LOCK_START - HEADER_LEN {
        offset + HEADER_LEN
    } else {
        offset + HEADER_LEN + LOCK_SPAN
    }
}

/// The database's size, for a file with a header that is `real` bytes long.
pub fn database_len(real: i64) -> i64 {
    if real <= HEADER_LEN {
        0
    } else if real <= LOCK_START {
        real - HEADER_LEN
    } else if real <= LOCK_START + LOCK_SPAN {
        LOCK_START - HEADER_LEN
    } else {
        real - HEADER_LEN - LOCK_SPAN
    }
}

/// A file control (outside SQLite's own numbers) that reads or writes the key header through the
/// connection's own file handle. Opening and closing another handle on a database SQLite has open
/// would drop this process's POSIX locks on it, so a live file's header is only ever touched this
/// way. The argument is a [`HeaderIo`].
pub const FCNTL_HEADER_IO: c_int = 0x4C42_4801;

/// The argument of [`FCNTL_HEADER_IO`]: `len` bytes at `offset` (inside the header) to or from `buf`.
#[repr(C)]
pub struct HeaderIo {
    pub write: bool,
    pub offset: i64,
    pub len: c_int,
    pub buf: *mut c_void,
}

/// One open file: our methods first (SQLite reads `pMethods` from here), then the default VFS's
/// own file object right after it in the same allocation.
#[repr(C)]
struct File {
    base: ffi::sqlite3_file,
    real: *mut ffi::sqlite3_file,
    /// Whether this is a main database with a key header (else every call passes through).
    shifted: bool,
}

/// The platform's default VFS, which does the real work; set once, before registering.
static INNER: AtomicPtr<ffi::sqlite3_vfs> = AtomicPtr::new(std::ptr::null_mut());
static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();

fn inner_vfs() -> *mut ffi::sqlite3_vfs {
    INNER.load(Ordering::Acquire)
}

/// Registers the VFS (once per process). Call before opening a connection with [`VFS_NAME`].
pub fn register() -> Result<(), String> {
    REGISTERED
        .get_or_init(|| {
            // SAFETY: plain calls into SQLite's C API. The VFS struct is leaked because SQLite keeps
            // the pointer for the life of the process; INNER is set before SQLite can call into it.
            unsafe {
                let rc = ffi::sqlite3_initialize();
                if rc != ffi::SQLITE_OK {
                    return Err(format!("FILE_VFS: SQLite failed to start ({rc})"));
                }
                let inner = ffi::sqlite3_vfs_find(std::ptr::null());
                if inner.is_null() {
                    return Err("FILE_VFS: SQLite has no default file system".into());
                }
                let ours = c_int::try_from(std::mem::size_of::<File>()).map_err(|e| e.to_string())?;
                INNER.store(inner, Ordering::Release);
                let vfs = Box::leak(Box::new(ffi::sqlite3_vfs {
                    iVersion: 2,
                    szOsFile: ours + (*inner).szOsFile,
                    mxPathname: (*inner).mxPathname,
                    pNext: std::ptr::null_mut(),
                    zName: VFS_NAME_C.as_ptr(),
                    pAppData: std::ptr::null_mut(),
                    xOpen: Some(x_open),
                    xDelete: Some(x_delete),
                    xAccess: Some(x_access),
                    xFullPathname: Some(x_full_pathname),
                    xDlOpen: Some(x_dl_open),
                    xDlError: Some(x_dl_error),
                    xDlSym: Some(x_dl_sym),
                    xDlClose: Some(x_dl_close),
                    xRandomness: Some(x_randomness),
                    xSleep: Some(x_sleep),
                    xCurrentTime: Some(x_current_time),
                    xGetLastError: Some(x_get_last_error),
                    xCurrentTimeInt64: Some(x_current_time_int64),
                    xSetSystemCall: None,
                    xGetSystemCall: None,
                    xNextSystemCall: None,
                }));
                let rc = ffi::sqlite3_vfs_register(vfs, 0);
                if rc != ffi::SQLITE_OK {
                    return Err(format!("FILE_VFS: could not register ({rc})"));
                }
                Ok(())
            }
        })
        .clone()
}

static IO_METHODS: ffi::sqlite3_io_methods = ffi::sqlite3_io_methods {
    // Version 2: shared memory (needed for WAL) but no memory-mapped pages, which would bypass
    // the offset.
    iVersion: 2,
    xClose: Some(io_close),
    xRead: Some(io_read),
    xWrite: Some(io_write),
    xTruncate: Some(io_truncate),
    xSync: Some(io_sync),
    xFileSize: Some(io_file_size),
    xLock: Some(io_lock),
    xUnlock: Some(io_unlock),
    xCheckReservedLock: Some(io_check_reserved_lock),
    xFileControl: Some(io_file_control),
    xSectorSize: Some(io_sector_size),
    xDeviceCharacteristics: Some(io_device_characteristics),
    xShmMap: Some(io_shm_map),
    xShmLock: Some(io_shm_lock),
    xShmBarrier: Some(io_shm_barrier),
    xShmUnmap: Some(io_shm_unmap),
    xFetch: None,
    xUnfetch: None,
};

/// The default VFS's file object inside ours.
unsafe fn real(file: *mut ffi::sqlite3_file) -> *mut ffi::sqlite3_file {
    (*file.cast::<File>()).real
}

unsafe fn shifted(file: *mut ffi::sqlite3_file) -> bool {
    (*file.cast::<File>()).shifted
}

/// Calls method `m` of the real file.
macro_rules! call {
    ($file:expr, $m:ident $(, $arg:expr)*) => {{
        let real = real($file);
        match (*(*real).pMethods).$m {
            Some(f) => f(real $(, $arg)*),
            None => ffi::SQLITE_IOERR,
        }
    }};
}

/// Whether a read or write of `len` bytes at database offset `offset` stays on one side of the
/// lock-byte gap (pages always do).
fn one_side(offset: i64, len: c_int) -> bool {
    let boundary = LOCK_START - HEADER_LEN;
    offset >= boundary || offset + i64::from(len) <= boundary
}

unsafe extern "C" fn x_open(_: *mut ffi::sqlite3_vfs, name: ffi::sqlite3_filename, file: *mut ffi::sqlite3_file, flags: c_int, out_flags: *mut c_int) -> c_int {
    let inner = inner_vfs();
    let ours = file.cast::<File>();
    let real = file.cast::<u8>().add(std::mem::size_of::<File>()).cast::<ffi::sqlite3_file>();
    (*ours).base.pMethods = std::ptr::null();
    (*ours).real = real;
    (*ours).shifted = false;
    let Some(open) = (*inner).xOpen else { return ffi::SQLITE_CANTOPEN };
    let rc = open(inner, name, real, flags, out_flags);
    if rc != ffi::SQLITE_OK {
        // SQLite won't call our xClose (pMethods is null), so close whatever the inner open left.
        if !(*real).pMethods.is_null() {
            if let Some(close) = (*(*real).pMethods).xClose {
                close(real);
            }
        }
        return rc;
    }
    if flags & ffi::SQLITE_OPEN_MAIN_DB != 0 {
        let mut head = [0u8; 16];
        let read = match (*(*real).pMethods).xRead {
            Some(read) => read(real, head.as_mut_ptr().cast::<c_void>(), 16, 0),
            None => ffi::SQLITE_IOERR,
        };
        (*ours).shifted = read == ffi::SQLITE_OK && &head == SIGNATURE;
    }
    (*ours).base.pMethods = &IO_METHODS;
    ffi::SQLITE_OK
}

unsafe extern "C" fn io_close(file: *mut ffi::sqlite3_file) -> c_int {
    call!(file, xClose)
}

unsafe extern "C" fn io_read(file: *mut ffi::sqlite3_file, buf: *mut c_void, len: c_int, offset: ffi::sqlite3_int64) -> c_int {
    if !shifted(file) {
        return call!(file, xRead, buf, len, offset);
    }
    if !one_side(offset, len) {
        return ffi::SQLITE_IOERR_READ;
    }
    call!(file, xRead, buf, len, real_offset(offset))
}

unsafe extern "C" fn io_write(file: *mut ffi::sqlite3_file, buf: *const c_void, len: c_int, offset: ffi::sqlite3_int64) -> c_int {
    if !shifted(file) {
        return call!(file, xWrite, buf, len, offset);
    }
    if !one_side(offset, len) {
        return ffi::SQLITE_IOERR_WRITE;
    }
    call!(file, xWrite, buf, len, real_offset(offset))
}

unsafe extern "C" fn io_truncate(file: *mut ffi::sqlite3_file, size: ffi::sqlite3_int64) -> c_int {
    let size = if shifted(file) { real_offset(size) } else { size };
    call!(file, xTruncate, size)
}

unsafe extern "C" fn io_sync(file: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    call!(file, xSync, flags)
}

unsafe extern "C" fn io_file_size(file: *mut ffi::sqlite3_file, size: *mut ffi::sqlite3_int64) -> c_int {
    let rc = call!(file, xFileSize, size);
    if rc == ffi::SQLITE_OK && shifted(file) {
        *size = database_len(*size);
    }
    rc
}

unsafe extern "C" fn io_lock(file: *mut ffi::sqlite3_file, level: c_int) -> c_int {
    call!(file, xLock, level)
}

unsafe extern "C" fn io_unlock(file: *mut ffi::sqlite3_file, level: c_int) -> c_int {
    call!(file, xUnlock, level)
}

unsafe extern "C" fn io_check_reserved_lock(file: *mut ffi::sqlite3_file, out: *mut c_int) -> c_int {
    call!(file, xCheckReservedLock, out)
}

/// SAFETY (FCNTL_HEADER_IO): `arg` points to a live [`HeaderIo`] whose `buf` holds `len` bytes.
unsafe extern "C" fn io_file_control(file: *mut ffi::sqlite3_file, op: c_int, arg: *mut c_void) -> c_int {
    // Memory-mapped reads are off without xFetch (methods version 2), so only the size hint, a size
    // in database terms, needs translating.
    if op == FCNTL_HEADER_IO {
        if !shifted(file) {
            return ffi::SQLITE_NOTFOUND;
        }
        let io = &*arg.cast::<HeaderIo>();
        if io.offset < 0 || io.len < 0 || io.offset + i64::from(io.len) > HEADER_LEN {
            return ffi::SQLITE_RANGE;
        }
        if !io.write {
            return call!(file, xRead, io.buf, io.len, io.offset);
        }
        let rc = call!(file, xWrite, io.buf.cast_const(), io.len, io.offset);
        return if rc == ffi::SQLITE_OK { call!(file, xSync, ffi::SQLITE_SYNC_FULL) } else { rc };
    }
    if op == ffi::SQLITE_FCNTL_SIZE_HINT && shifted(file) {
        let mut hint = real_offset(*arg.cast::<ffi::sqlite3_int64>());
        return call!(file, xFileControl, op, std::ptr::addr_of_mut!(hint).cast::<c_void>());
    }
    call!(file, xFileControl, op, arg)
}

unsafe extern "C" fn io_sector_size(file: *mut ffi::sqlite3_file) -> c_int {
    call!(file, xSectorSize)
}

unsafe extern "C" fn io_device_characteristics(file: *mut ffi::sqlite3_file) -> c_int {
    call!(file, xDeviceCharacteristics)
}

unsafe extern "C" fn io_shm_map(file: *mut ffi::sqlite3_file, page: c_int, size: c_int, extend: c_int, out: *mut *mut c_void) -> c_int {
    call!(file, xShmMap, page, size, extend, out)
}

unsafe extern "C" fn io_shm_lock(file: *mut ffi::sqlite3_file, offset: c_int, n: c_int, flags: c_int) -> c_int {
    call!(file, xShmLock, offset, n, flags)
}

unsafe extern "C" fn io_shm_barrier(file: *mut ffi::sqlite3_file) {
    let real = real(file);
    if let Some(f) = (*(*real).pMethods).xShmBarrier {
        f(real);
    }
}

unsafe extern "C" fn io_shm_unmap(file: *mut ffi::sqlite3_file, delete: c_int) -> c_int {
    call!(file, xShmUnmap, delete)
}

/// Calls VFS method `m` of the default VFS.
macro_rules! inner_call {
    ($m:ident, $default:expr $(, $arg:expr)*) => {{
        let inner = inner_vfs();
        match (*inner).$m {
            Some(f) => f(inner $(, $arg)*),
            None => $default,
        }
    }};
}

unsafe extern "C" fn x_delete(_: *mut ffi::sqlite3_vfs, name: *const c_char, sync: c_int) -> c_int {
    inner_call!(xDelete, ffi::SQLITE_IOERR_DELETE, name, sync)
}

unsafe extern "C" fn x_access(_: *mut ffi::sqlite3_vfs, name: *const c_char, flags: c_int, out: *mut c_int) -> c_int {
    inner_call!(xAccess, ffi::SQLITE_IOERR_ACCESS, name, flags, out)
}

unsafe extern "C" fn x_full_pathname(_: *mut ffi::sqlite3_vfs, name: *const c_char, n: c_int, out: *mut c_char) -> c_int {
    inner_call!(xFullPathname, ffi::SQLITE_CANTOPEN, name, n, out)
}

unsafe extern "C" fn x_dl_open(_: *mut ffi::sqlite3_vfs, name: *const c_char) -> *mut c_void {
    inner_call!(xDlOpen, std::ptr::null_mut(), name)
}

unsafe extern "C" fn x_dl_error(_: *mut ffi::sqlite3_vfs, n: c_int, out: *mut c_char) {
    inner_call!(xDlError, (), n, out)
}

unsafe extern "C" fn x_dl_sym(_: *mut ffi::sqlite3_vfs, handle: *mut c_void, name: *const c_char) -> Option<unsafe extern "C" fn(*mut ffi::sqlite3_vfs, *mut c_void, *const c_char)> {
    inner_call!(xDlSym, None, handle, name)
}

unsafe extern "C" fn x_dl_close(_: *mut ffi::sqlite3_vfs, handle: *mut c_void) {
    inner_call!(xDlClose, (), handle)
}

unsafe extern "C" fn x_randomness(_: *mut ffi::sqlite3_vfs, n: c_int, out: *mut c_char) -> c_int {
    inner_call!(xRandomness, 0, n, out)
}

unsafe extern "C" fn x_sleep(_: *mut ffi::sqlite3_vfs, micros: c_int) -> c_int {
    inner_call!(xSleep, 0, micros)
}

unsafe extern "C" fn x_current_time(_: *mut ffi::sqlite3_vfs, out: *mut f64) -> c_int {
    inner_call!(xCurrentTime, ffi::SQLITE_ERROR, out)
}

unsafe extern "C" fn x_get_last_error(_: *mut ffi::sqlite3_vfs, n: c_int, out: *mut c_char) -> c_int {
    inner_call!(xGetLastError, 0, n, out)
}

unsafe extern "C" fn x_current_time_int64(_: *mut ffi::sqlite3_vfs, out: *mut ffi::sqlite3_int64) -> c_int {
    inner_call!(xCurrentTimeInt64, ffi::SQLITE_ERROR, out)
}

#[cfg(test)]
mod tests;
