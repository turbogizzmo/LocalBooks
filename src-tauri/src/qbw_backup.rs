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
mod tests;
