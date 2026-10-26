use super::*;

#[test]
fn decodes_windows_1252_utf8_and_utf16() {
    assert_eq!(decode(b"Caf\xe9 \x93Q\x94 \x80"), "Café “Q” €");
    assert_eq!(decode("Café".as_bytes()), "Café");
    assert_eq!(decode(b"\xEF\xBB\xBFok"), "ok");
    assert_eq!(decode(b"\xFF\xFEo\0k\0"), "ok");
}

#[test]
fn quantities_scale_by_ten_thousand() {
    assert_eq!(quantity_e4("1.5"), Some(15_000));
    assert_eq!(quantity_e4("-2"), Some(-20_000));
    assert_eq!(quantity_e4("0.0001"), Some(1));
    assert_eq!(quantity_e4("1.00005"), None);
    assert_eq!(quantity_e4("abc"), None);
}

#[test]
fn subaccounts_get_their_parents() {
    let mut accounts = Accounts::default();
    let child = accounts.ensure("Construction:Labor", "INCOME");
    let parent = accounts.id("construction").unwrap();
    assert_eq!(accounts.list.iter().find(|a| a.id == child).unwrap().parent_id, Some(parent));
    assert_eq!(accounts.declare("Construction", "EXPENSE", true), parent);
    assert_eq!(accounts.kind(parent), "EXPENSE");
}

#[test]
fn reads_terms() {
    assert_eq!(terms_days("Net 30"), Some(30));
    assert_eq!(terms_days("2% 10 Net 30"), Some(30));
    assert_eq!(terms_days("Due on receipt"), Some(0));
    assert_eq!(terms_days("Consignment"), None);
}
