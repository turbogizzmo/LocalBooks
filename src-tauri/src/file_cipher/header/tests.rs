use super::*;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("localbooks-file-header-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn slot(username: &str) -> HeaderSlot {
    HeaderSlot {
        kind: "PASSWORD".into(),
        user_id: Some(1),
        username: Some(username.into()),
        memory_kib: 65536,
        iterations: 3,
        parallelism: 1,
        salt: "c2FsdA==".into(),
        nonce: "bm9uY2U=".into(),
        wrapped_key: "d3JhcHBlZA==".into(),
    }
}

fn header(username: &str) -> Header {
    Header { file_id: String::new(), page_size: 4096, slots: vec![slot(username)] }
}

// The layout fits: after the signature, the copies don't overlap, and both fit in the header.
const _: () = assert!(COPY_A >= 16 && COPY_A + COPY_LEN <= COPY_B && COPY_B + COPY_LEN <= HEADER_LEN as u64);

#[test]
fn a_new_header_reads_back_and_marks_the_file() {
    let dir = scratch("new");
    let path = dir.join("books.db");
    create(&path, &header("ann")).unwrap();
    assert!(is_encrypted(&path));
    assert_eq!(read(&path).unwrap(), header("ann"));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), u64::try_from(HEADER_LEN).unwrap());
    assert!(create(&path, &header("ann")).is_err(), "never over an existing file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writes_alternate_copies_and_a_torn_copy_falls_back_to_the_other() {
    let dir = scratch("rotate");
    let path = dir.join("books.db");
    create(&path, &header("v1")).unwrap();
    write(&path, &header("v2")).unwrap();
    assert_eq!(read(&path).unwrap(), header("v2"));
    write(&path, &header("v3")).unwrap();
    assert_eq!(read(&path).unwrap(), header("v3"));

    // Tear whichever copy holds v3: the other (v2) is read instead.
    let mut file = OpenOptions::new().read(true).write(true).open(&path).unwrap();
    let [a, b] = read_both(&mut file);
    let newest_at = if a.as_ref().map(|c| c.0) > b.as_ref().map(|c| c.0) { COPY_A } else { COPY_B };
    file.seek(SeekFrom::Start(newest_at + 100)).unwrap();
    file.write_all(&[0xFF; 64]).unwrap();
    drop(file);
    assert_eq!(read(&path).unwrap(), header("v2"), "the older whole copy survives");

    // The next write goes over the torn copy, leaving v2 whole.
    write(&path, &header("v4")).unwrap();
    assert_eq!(read(&path).unwrap(), header("v4"));
    let mut file = File::open(&path).unwrap();
    let copies: Vec<_> = read_both(&mut file).into_iter().flatten().map(|(_, h)| h.slots[0].username.clone().unwrap()).collect();
    assert_eq!(copies.len(), 2);
    assert!(copies.contains(&"v2".to_string()) && copies.contains(&"v4".to_string()));

    // Both copies damaged: refused, never guessed.
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    for at in [COPY_A, COPY_B] {
        file.seek(SeekFrom::Start(at)).unwrap();
        file.write_all(b"XXXXXXXX").unwrap();
    }
    drop(file);
    assert!(read(&path).unwrap_err().starts_with("FILE_HEADER_DAMAGED"));
    assert!(write(&path, &header("v5")).unwrap_err().starts_with("FILE_HEADER_DAMAGED"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plain_files_and_oversized_headers_are_refused() {
    let dir = scratch("refuse");
    let plain = dir.join("plain.db");
    std::fs::write(&plain, b"SQLite format 3\0rest").unwrap();
    assert!(!is_encrypted(&plain));
    assert!(read(&plain).unwrap_err().starts_with("FILE_NOT_ENCRYPTED"));
    assert!(write(&plain, &header("x")).unwrap_err().starts_with("FILE_NOT_ENCRYPTED"));
    assert_eq!(std::fs::read(&plain).unwrap(), b"SQLite format 3\0rest", "a plain file is never written to");

    let huge = Header { file_id: String::new(), page_size: 4096, slots: (0..1000).map(|i| slot(&format!("user{i}"))).collect() };
    assert!(create(&dir.join("huge.db"), &huge).unwrap_err().starts_with("FILE_HEADER_FULL"));
    assert!(!dir.join("huge.db").exists(), "nothing is left behind");
    let _ = std::fs::remove_dir_all(&dir);
}
