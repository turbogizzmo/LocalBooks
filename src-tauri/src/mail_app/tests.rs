use super::*;

#[test]
fn recipients_must_be_plain_addresses() {
    assert_eq!(parse_recipients(" a@x.com; b.c@y.co.uk ,").unwrap(), vec!["a@x.com", "b.c@y.co.uk"]);
    assert!(parse_recipients("").is_err());
    for bad in ["nobody", "a@b", "a b@x.com", "Name <a@x.com>", "a@x.com\nBcc: c@y.com", "@x.com", "a@.com"] {
        assert!(parse_recipients(bad).is_err(), "{bad:?} should be refused");
    }
}

#[test]
fn mailto_links_encode_everything() {
    let message = Compose {
        to: vec!["ap@initech.test".into(), "o'neil+ar@x.com".into()],
        subject: "Invoice 1001 & more".into(),
        body: "Hi,\nTotal: $5 = 100%".into(),
        attachment: PathBuf::from("/tmp/Invoice 1001.pdf"),
        more: vec![PathBuf::from("/tmp/W-9.pdf")],
    };
    assert_eq!(message.files(), [Path::new("/tmp/Invoice 1001.pdf"), Path::new("/tmp/W-9.pdf")], "the form first");
    assert_eq!(
        mailto_url(&message),
        "mailto:ap@initech.test,o%27neil%2Bar@x.com?subject=Invoice%201001%20%26%20more&body=Hi%2C%0D%0ATotal%3A%20%245%20%3D%20100%25"
    );
}

#[test]
fn attachments_get_their_own_folder() {
    let dir = std::env::temp_dir().join(format!("localbooks-outgoing-test-{}", std::process::id()));
    let a = stage_attachment(&dir, "Invoice 1.pdf", b"%PDF-a").unwrap();
    let b = stage_attachment(&dir, "Invoice 1.pdf", b"%PDF-b").unwrap();
    assert_ne!(a, b);
    assert_eq!(std::fs::read(&a).unwrap(), b"%PDF-a");
    assert_eq!(b.file_name().unwrap(), "Invoice 1.pdf");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn several_files_share_one_folder_without_overwriting_each_other() {
    let dir = std::env::temp_dir().join(format!("localbooks-outgoing-many-{}", std::process::id()));
    let staged = stage_files(&dir, &[("Invoice 1.pdf", b"%PDF-form"), ("photo.jpg", b"jpg"), ("photo.jpg", b"jpg2")]).unwrap();
    assert_eq!(staged.len(), 3);
    assert!(staged.iter().all(|p| p.parent() == staged[0].parent()), "one folder per message");
    assert_eq!(staged[2].file_name().unwrap(), "photo (2).jpg");
    assert_eq!(std::fs::read(&staged[2]).unwrap(), b"jpg2");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// An attachment's name comes from the company file, which someone else may have written: a name
/// that is a path must not put the file outside the staging folder.
#[test]
fn a_stored_name_that_is_a_path_stays_inside_the_staging_folder() {
    let dir = std::env::temp_dir().join(format!("localbooks-outgoing-paths-{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!("localbooks-escaped-{}.txt", std::process::id()));
    let absolute = outside.to_string_lossy().into_owned();
    let climbing = format!("../../{}", outside.file_name().unwrap().to_string_lossy());
    let staged = stage_files(&dir, &[(climbing.as_str(), b"a"), (absolute.as_str(), b"b"), ("..\\..\\evil.bat", b"c"), ("..", b"d"), ("", b"e")]).unwrap();
    assert!(!outside.exists(), "nothing was written outside the staging folder");
    let folder = staged[0].parent().unwrap().to_path_buf();
    for path in &staged {
        assert_eq!(path.parent().unwrap(), folder, "{}", path.display());
        assert!(path.is_file());
    }
    assert_eq!(staged[2].file_name().unwrap(), "evil.bat");
    assert_eq!(staged[3].file_name().unwrap(), "attachment");
    assert_eq!(staged[4].file_name().unwrap(), "attachment (2)");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn staged_names_keep_ordinary_names_and_their_extensions() {
    assert_eq!(staged_name("Invoice 1.pdf"), "Invoice 1.pdf");
    assert_eq!(staged_name("W-9 (signed).PDF"), "W-9 (signed).PDF");
    assert_eq!(staged_name("C:\\Users\\me\\receipt.jpg"), "receipt.jpg");
    assert_eq!(staged_name("a:b*c?.txt"), "a_b_c_.txt");
    assert_eq!(staged_name(".hidden"), "hidden");
    assert_eq!(staged_name("..."), "attachment");
    let long = format!("{}.pdf", "x".repeat(400));
    let cut = staged_name(&long);
    assert_eq!(cut.chars().count(), 124);
    assert!(cut.ends_with(".pdf"));
}
