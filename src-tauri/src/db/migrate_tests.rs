use super::*;

/// Released migrations are frozen: a company file that already applied one refuses to open if
/// its SQL changes (sqlx compares checksums), which would stop the app for every existing user.
/// Add a line here for each new migration; never edit an existing line or migration file.
#[tokio::test]
async fn a_file_upgraded_by_a_newer_version_is_refused_plainly_and_left_alone() {
    let dir = std::env::temp_dir().join(format!("localbooks-newer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("books.db");
    let pool = super::open(&path).await.unwrap();
    sqlx::query("INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (9999, 'from the future', 1, x'00', 0)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let err = super::open(&path).await.unwrap_err();
    assert!(err.starts_with("DB_NEWER_FILE"), "{err}");
    assert!(err.contains("Install the latest LocalBooks"), "{err}");
    let pool = super::open_read_only_for_test(&path).await;
    let newest: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations").fetch_one(&pool).await.unwrap();
    assert_eq!(newest, 9999, "nothing was rolled back or rewritten");
    pool.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn released_migrations_are_never_edited() {
    const RELEASED: &[(i64, &str)] = &[
        (1, "d5a36071e057949f32a00e72"),
        (2, "fbf4c17dd5b3397f7d420e4a"),
        (3, "96723d0cb1d74eb159ec0b22"),
        (4, "6fe4f2ad900efc25aabe2070"),
        (5, "b0b3d32999119aba01f28d70"),
        (6, "dfab9cfc132bb83f4bb8d98a"),
        (7, "19148d3cbc6ce0e378e3ef1a"),
        (8, "d8c7cbdcbc46e8b5b6984106"),
        (9, "6bc8df9779eb42ec784d6a00"),
        (10, "427c757dc2359e67b42e97a2"),
        (11, "61ff0225af9d3a53253a4be2"),
        (12, "b3e722249b0ca4d444f0174a"),
        (13, "6257a8ded273435ec2e96f7e"),
        (14, "f48284e33a4d4bfa1a91cc9c"),
        (15, "f8145154c4a9384330d213fc"),
        (16, "eae3d1c0215ed3d477b787f3"),
        (17, "a778f93e4c0eb1b73f01adc7"),
        (18, "2ff4875ee63dff78a07fe2ea"),
        (19, "a8d9c49bcc2e7b9ea1ae92ba"),
        (20, "ef6f5c20721a265c4926c12d"),
        (21, "4d7f42ef0674c56168e39ac5"),
        (22, "02521200fbf68600e32719a5"),
        (23, "19280628bd7aecf2b9a34ea8"),
        (24, "61bad942842dbe5b8742f1b0"),
        (25, "8859bd8099ef6634d7a4ede0"),
        (26, "67a7dae27b5c18e776bc0d8c"),
        (27, "d4dd8015d3504efa8e48fa85"),
        (28, "35bc8727729cb4fa59546f48"),
        (29, "52841401ff83c5b387edf443"),
        (30, "9c0be4861701d989776879ce"),
        (31, "7c46a93c8c9ebc9be3cfdc5f"),
        (32, "7bc45918086695080c6dbd96"),
        (33, "56b136669141e6d545b88b7b"),
        (34, "b9bb6b77a78ebbc83b7b878f"),
        (35, "74a6e4a4be6a78e1dc322e9a"),
        (36, "d307ef0d4f24a13e8ab2065e"),
        (37, "2da2eb42ab82f41f19fd011d"),
        (38, "3e495b0ee917b7be5d03953b"),
        (39, "b97e9871736181cd0f4b33c5"),
        (40, "7895f21efb758ee2daf020be"),
        (41, "cf3d7741aaa74c331ba2b93e"),
        (42, "02edb65210048bf6d56b7aaf"),
        (43, "a3c2aae0cee51bb815cede4b"),
        (44, "2eb3f96cc028ff0f7a2ef7cd"),
        (45, "ad79e6b147ac00c8f5a22a9b"),
        (46, "99ef8855fba6968cc3282ede"),
        (48, "14dd6e91430541f3ebd7b55d"),
        (47, "8de3ae35947f2b2839a8455c"),
    ];
    let actual: Vec<(i64, String)> = MIGRATOR
        .iter()
        .map(|m| (m.version, m.checksum.iter().take(12).map(|b| format!("{b:02x}")).collect()))
        .collect();
    for (version, pinned) in RELEASED {
        let found = actual.iter().find(|(v, _)| v == version).unwrap_or_else(|| panic!("migration {version} was removed"));
        assert_eq!(&found.1, pinned, "migration {version} was edited after release; add a new migration instead");
    }
    for (version, checksum) in &actual {
        assert!(
            RELEASED.iter().any(|(v, _)| v == version),
            "new migration {version}: once it ships, pin it here as ({version}, \"{checksum}\")"
        );
    }
}

/// Upgrade check for a real company file: point LOCALBOOKS_UPGRADE_CHECK at a COPY of one and
/// run `cargo test -- --ignored upgrades_a_copy`. The copy is migrated in place and verified.
#[tokio::test]
#[ignore]
async fn upgrades_a_copy_of_a_real_company_file() {
    let path = std::env::var("LOCALBOOKS_UPGRADE_CHECK").expect("set LOCALBOOKS_UPGRADE_CHECK to a copied company file");
    let path = Path::new(&path);
    let before = crate::backup::inspect(path).await.expect("the copy is a valid company file");
    let pool = open(path).await.expect("migrations apply");
    let verification = crate::ledger::verify_ledger_audit(&pool).await.unwrap();
    let tb = crate::reports::trial_balance(&pool).await.unwrap();
    println!(
        "upgraded from schema {:?}: {} posted entries, intact = {}, trial balance {} / {}",
        before.schema_version, verification.entries_checked, verification.intact, tb.total_debit_cents, tb.total_credit_cents
    );
    assert!(verification.intact, "{:?}", verification.findings);
    assert_eq!(tb.total_debit_cents, tb.total_credit_cents);
    pool.close().await;
}

#[tokio::test]
async fn every_migration_is_recorded_once_in_the_audit_trail() {
    let pool = test_pool().await;
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1").fetch_one(&pool).await.unwrap();
    let logged = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'MIGRATION'").fetch_one(&pool).await.unwrap()
    };
    assert_eq!(logged().await, applied);
    migrate(&pool).await.unwrap();
    assert_eq!(logged().await, applied, "re-running migrations adds no duplicate events");
    let latest: String = sqlx::query_scalar(
        "SELECT json_extract(payload_snapshot, '$.description') FROM ledger_audit_log
             WHERE action_type = 'MIGRATION' ORDER BY target_entity_id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let newest = MIGRATOR.iter().map(|m| (m.version, m.description.to_string())).max().unwrap().1;
    assert_eq!(latest, newest, "the newest migration is recorded with its description");
}
