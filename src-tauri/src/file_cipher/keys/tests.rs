use super::*;
use crate::file_cipher::header::Header;

fn header(slots: Vec<HeaderSlot>) -> Header {
    Header { file_id: "file-a".into(), page_size: 4096, slots }
}

#[tokio::test]
async fn slots_open_with_their_secret_only() {
    let key = PageKey::generate().unwrap();
    let ann = password_slot(&key, "file-a", 1, "Ann", "correct horse 1").await.unwrap();
    let recovery = recovery_slot(&key, "file-a", "ABCD-EFGH-JKMN-PQRS-TVWX-YZ01-2345-6789").await.unwrap();
    let h = header(vec![ann.clone(), recovery]);

    let (opened, id) = unlock_with_password(&h, " ann ", "correct horse 1").await.unwrap().expect("the right password, any case");
    assert_eq!((opened.bytes(), id), (key.bytes(), 1));
    assert!(unlock_with_password(&h, "ann", "wrong password").await.unwrap().is_none());
    assert!(unlock_with_password(&h, "nobody", "correct horse 1").await.unwrap().is_none(), "an unknown name opens nothing");
    let by_recovery = unlock_with_recovery_key(&h, "abcd efgh jkmn pqrs tvwx yz01 2345 6789").await.unwrap().expect("however it is typed");
    assert_eq!(by_recovery.bytes(), key.bytes());
    assert!(unlock_with_recovery_key(&h, "ABCD-EFGH-JKMN-PQRS-TVWX-YZ01-2345-678A").await.unwrap().is_none());

    // A slot moved to another file, or to another user, doesn't open.
    let moved = Header { file_id: "file-b".into(), ..h.clone() };
    assert!(unlock_with_password(&moved, "ann", "correct horse 1").await.unwrap().is_none());
    let renamed = HeaderSlot { user_id: Some(2), username: Some("bob".into()), ..ann };
    assert!(unlock_with_password(&header(vec![renamed]), "bob", "correct horse 1").await.unwrap().is_none());
}

#[tokio::test]
async fn slot_lists_change_as_users_do() {
    let key = PageKey::generate().unwrap();
    let old = password_slot(&key, "file-a", 1, "ann", "old password 1").await.unwrap();
    let bob = password_slot(&key, "file-a", 2, "bob", "bob password 1").await.unwrap();
    let h = header(vec![old, bob]);
    let new = password_slot(&key, "file-a", 1, "ann", "new password 1").await.unwrap();
    let both = adding_user_slot(&h, new.clone());
    assert!(unlock_with_password(&both, "ann", "old password 1").await.unwrap().is_some(), "while a change is saved, the old password still opens");
    assert!(unlock_with_password(&both, "ann", "new password 1").await.unwrap().is_some());
    let replaced = with_user_slot(&h, 1, new);
    assert!(unlock_with_password(&replaced, "ann", "old password 1").await.unwrap().is_none());
    assert_eq!(replaced.slots.iter().filter(|s| s.user_id == Some(1)).count(), 1);
    let off = without_user(&replaced, 1);
    assert!(unlock_with_password(&off, "ann", "new password 1").await.unwrap().is_none());
    assert!(unlock_with_password(&off, "bob", "bob password 1").await.unwrap().is_some(), "others keep theirs");
}

#[test]
fn a_page_key_never_prints() {
    let key = PageKey::from_bytes([0xAB; 32]);
    assert_eq!(format!("{key:?}"), "PageKey(..)");
}
