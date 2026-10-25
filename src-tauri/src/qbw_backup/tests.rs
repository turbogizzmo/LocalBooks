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
