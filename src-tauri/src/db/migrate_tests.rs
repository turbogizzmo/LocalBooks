use super::*;

/// Released migrations are frozen: a company file that already applied one refuses to open if
/// its SQL changes (sqlx compares checksums), which would stop the app for every existing user.
/// Pin each new migration (see `released_migrations_are_never_edited`); never edit a pin or migration file.
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

/// The pins live one file per migration in `src/db/released_migrations/` (`0049.checksum` holds
/// migration 49's), so two branches that each add a migration never touch the same file.
#[test]
fn released_migrations_are_never_edited() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/db/released_migrations");
    let mut released: Vec<(i64, String)> = std::fs::read_dir(&dir)
        .expect("read src/db/released_migrations")
        .map(|entry| {
            let path = entry.expect("read a folder entry").path();
            let version = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse().ok());
            let version = version.unwrap_or_else(|| panic!("{} is not named <version>.checksum", path.display()));
            (version, std::fs::read_to_string(&path).expect("read a checksum pin").trim().to_string())
        })
        .collect();
    released.sort();
    assert!(released.len() > 40, "found the pins in {}", dir.display());
    let actual: Vec<(i64, String)> = MIGRATOR
        .iter()
        .map(|m| (m.version, m.checksum.iter().take(12).map(|b| format!("{b:02x}")).collect()))
        .collect();
    for (version, pinned) in &released {
        let found = actual.iter().find(|(v, _)| v == version).unwrap_or_else(|| panic!("migration {version} was removed"));
        assert_eq!(&found.1, pinned, "migration {version} was edited after release; add a new migration instead");
    }
    for (version, checksum) in &actual {
        assert!(
            released.iter().any(|(v, _)| v == version),
            "new migration {version}: once it ships, pin it in src/db/released_migrations/{version:04}.checksum containing {checksum}"
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
