//! Unpacks desktop backup files (.qbb) into the company file they contain.
//!
//! A backup is an OLE compound file. The company file is its `file_COMPANY_FILE` stream,
//! compressed with zlib (the `Compression_Header` stream names the format); the other streams
//! hold templates, print settings and the like, which an import does not need.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const COMPANY_STREAM: &str = "file_COMPANY_FILE";
const COMPRESSION_STREAM: &str = "Compression_Header";
/// Refuse to inflate past this, so a damaged or hostile backup cannot fill the disk.
const MAX_COMPANY_FILE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// A company file unpacked from a backup into a temporary file, deleted when dropped.
#[derive(Debug)]
pub struct Unpacked {
    pub path: PathBuf,
}

impl Drop for Unpacked {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn is_backup(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("qbb"))
}

/// Unpacks the company file from `backup` into `dir`. The backup is only read.
pub fn unpack(backup: &Path, dir: &Path) -> Result<Unpacked, String> {
    let not_backup = |detail: String| format!("QBW_BAD_BACKUP: {} is not a readable backup file ({detail})", backup.display());
    let mut container = cfb::open(backup).map_err(|e| not_backup(e.to_string()))?;

    if let Ok(mut header) = container.open_stream(COMPRESSION_STREAM) {
        let mut text = String::new();
        let _ = header.read_to_string(&mut text);
        if !text.is_empty() && !text.contains("Deflate_ZLIB") {
            return Err(format!("QBW_BACKUP_FORMAT: {} uses an unsupported compression ({})", backup.display(), text.trim_end_matches('\0')));
        }
    }
    let stream = container.open_stream(COMPANY_STREAM).map_err(|_| not_backup("it holds no company file".into()))?;

    std::fs::create_dir_all(dir).map_err(|e| format!("QBW_UNPACK: could not create {}: {e}", dir.display()))?;
    let path = dir.join(format!("localbooks-unpacked-{}-{}.qbw", std::process::id(), unique_suffix()));
    let unpacked = Unpacked { path };
    let mut out = std::fs::File::create(&unpacked.path).map_err(|e| format!("QBW_UNPACK: could not write {}: {e}", unpacked.path.display()))?;
    let mut inflater = flate2::read::ZlibDecoder::new(stream).take(MAX_COMPANY_FILE_BYTES + 1);
    let written = std::io::copy(&mut inflater, &mut out).map_err(|e| not_backup(format!("the company file does not decompress: {e}")))?;
    if written > MAX_COMPANY_FILE_BYTES {
        return Err(not_backup("the company file is implausibly large".into()));
    }
    out.flush().map_err(|e| format!("QBW_UNPACK: {e}"))?;
    Ok(unpacked)
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("localbooks-qbb-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_backup(path: &Path, company: &[u8]) {
        let mut c = cfb::create(path).unwrap();
        c.create_stream(COMPRESSION_STREAM).unwrap().write_all(b"Compression_Format=Deflate_ZLIB;Version=1.2.3;").unwrap();
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(company).unwrap();
        c.create_stream(COMPANY_STREAM).unwrap().write_all(&z.finish().unwrap()).unwrap();
        c.flush().unwrap();
    }

    #[test]
    fn unpacks_the_company_stream_and_cleans_up() {
        let dir = scratch("ok");
        let backup = dir.join("Books.QBB");
        let company: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
        write_backup(&backup, &company);
        assert!(is_backup(&backup));

        let unpacked = unpack(&backup, &dir).unwrap();
        assert_eq!(std::fs::read(&unpacked.path).unwrap(), company);
        let path = unpacked.path.clone();
        drop(unpacked);
        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_files_that_are_not_backups() {
        let dir = scratch("bad");
        let fake = dir.join("fake.qbb");
        std::fs::write(&fake, b"PK\x03\x04 not a compound file").unwrap();
        assert!(unpack(&fake, &dir).unwrap_err().starts_with("QBW_BAD_BACKUP"));

        let empty = dir.join("empty.qbb");
        cfb::create(&empty).unwrap().flush().unwrap();
        assert!(unpack(&empty, &dir).unwrap_err().contains("no company file"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Unpacks a real backup (read-only): set LOCALBOOKS_QBB_CHECK to its path.
    #[test]
    #[ignore]
    fn unpacks_a_real_backup() {
        let path = std::env::var("LOCALBOOKS_QBB_CHECK").expect("set LOCALBOOKS_QBB_CHECK");
        let dir = scratch("real");
        let unpacked = unpack(Path::new(&path), &dir).unwrap();
        let len = std::fs::metadata(&unpacked.path).unwrap().len();
        println!("unpacked {len} bytes ({} pages)", len / 4096);
        assert_eq!(len % 4096, 0);
    }
}
