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
