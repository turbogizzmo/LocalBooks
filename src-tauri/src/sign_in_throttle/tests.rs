use super::*;

const FILE: &str = "key-1";

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds)
}

fn fail(t: &Throttle, who: &str, times: u32, now: SystemTime) {
    for _ in 0..times {
        t.failed(FILE, who, now);
    }
}

#[test]
fn the_wait_grows_and_never_passes_five_minutes() {
    assert_eq!((1..=FREE_TRIES).map(wait_after).max(), Some(Duration::ZERO), "the first tries are free");
    assert_eq!(wait_after(FREE_TRIES + 1), Duration::from_secs(30));
    assert_eq!(wait_after(FREE_TRIES + 2), Duration::from_secs(60));
    assert_eq!(wait_after(FREE_TRIES + 3), Duration::from_secs(120));
    assert_eq!(wait_after(FREE_TRIES + 4), MAX_WAIT);
    assert_eq!(wait_after(u32::MAX), MAX_WAIT, "no lockout, however many tries");
}

#[test]
fn a_wait_starts_after_the_free_tries_and_ends_on_time() {
    let t = Throttle::new(None);
    fail(&t, "ann", FREE_TRIES, at(0));
    assert!(t.check(FILE, "ann", at(0)).is_ok(), "four wrong tries cost nothing");
    fail(&t, "ann", 1, at(0));
    let refused = t.check(FILE, "ann", at(10)).unwrap_err();
    assert!(refused.starts_with("SIGN_IN_WAIT") && refused.contains("20 seconds"), "{refused}");
    assert!(t.check(FILE, "ann", at(30)).is_ok(), "after the wait, anyone may try again");

    fail(&t, "ann", 10, at(100));
    let refused = t.check(FILE, "ann", at(100)).unwrap_err();
    assert!(refused.contains("5 minutes"), "{refused}");
    assert!(t.check(FILE, "ann", at(100 + 300)).is_ok(), "never longer than five minutes");
}

#[test]
fn counts_are_per_user_and_per_file_and_a_success_clears_one() {
    let t = Throttle::new(None);
    fail(&t, "Ann", FREE_TRIES + 1, at(0));
    assert!(t.check(FILE, " ann ", at(1)).is_err(), "names compare without case or spaces, like the users table");
    assert!(t.check(FILE, "bob", at(1)).is_ok(), "another user (say, the other Admin) can still sign in");
    assert!(t.check("key-2", "ann", at(1)).is_ok(), "another company file has its own count");
    assert!(t.check(FILE, RECOVERY, at(1)).is_ok(), "the recovery key has its own count");
    t.succeeded(FILE, "ANN");
    assert!(t.check(FILE, "ann", at(1)).is_ok());
    fail(&t, "ann", FREE_TRIES, at(2));
    assert!(t.check(FILE, "ann", at(2)).is_ok(), "the count started again from zero");
}

#[test]
fn old_counts_expire_and_a_clock_set_back_doesnt_lengthen_the_wait() {
    let t = Throttle::new(None);
    fail(&t, "ann", FREE_TRIES + 4, at(0));
    fail(&t, "bob", 1, at(2 * 24 * 60 * 60));
    assert!(!t.counts.lock().unwrap().contains_key(&Throttle::key(FILE, "ann")), "a day without failures forgets the count");

    fail(&t, "cy", FREE_TRIES + 4, at(1000));
    assert!(t.check(FILE, "cy", at(10)).is_ok(), "a clock moved back never strands anyone");
}

#[test]
fn counts_survive_a_restart_and_an_unwritable_folder_never_blocks() {
    let dir = std::env::temp_dir().join(format!("localbooks-sign-in-waits-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let first = Throttle::new(Some(&dir));
    fail(&first, "ann", FREE_TRIES + 1, SystemTime::now());
    let saved = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
    assert!(!saved.contains("password"), "only names and counts are saved");
    let after_restart = Throttle::new(Some(&dir));
    assert!(after_restart.check(FILE, "ann", SystemTime::now()).is_err(), "restarting LocalBooks doesn't reset the wait");
    let _ = std::fs::remove_dir_all(&dir);

    // A settings "folder" that is really a file: nothing can be saved, the count still works.
    let blocked = std::env::temp_dir().join(format!("localbooks-sign-in-waits-file-{}", std::process::id()));
    std::fs::write(&blocked, b"not a folder").unwrap();
    let t = Throttle::new(Some(&blocked));
    fail(&t, "ann", FREE_TRIES + 1, SystemTime::now());
    assert!(t.check(FILE, "ann", SystemTime::now()).is_err());
    let _ = std::fs::remove_file(&blocked);
}

#[tokio::test]
async fn attempts_are_refused_counted_and_cleared() {
    let t = Throttle::new(None);
    let wrong = || async { Err::<(), String>("SIGN_IN_FAILED: the user name or password is not correct".into()) };
    let rule = || async { Err::<(), String>("PASSWORD_TOO_SHORT: use at least 8 characters".into()) };
    let counts = |e: &str| e.starts_with("SIGN_IN_FAILED");
    for _ in 0..FREE_TRIES {
        assert!(t.attempt(FILE, "ann", counts, wrong()).await.unwrap_err().starts_with("SIGN_IN_FAILED"));
    }
    for _ in 0..10 {
        assert!(t.attempt(FILE, "ann", counts, rule()).await.unwrap_err().starts_with("PASSWORD_TOO_SHORT"), "errors that aren't guesses don't count");
    }
    assert!(t.attempt(FILE, "ann", counts, async { Ok(()) }).await.is_ok(), "still within the free tries");
    for _ in 0..=FREE_TRIES {
        let _ = t.attempt(FILE, "ann", counts, wrong()).await;
    }
    let mut ran = false;
    let refused = t
        .attempt(FILE, "ann", counts, async {
            ran = true;
            Ok(())
        })
        .await;
    assert!(refused.unwrap_err().starts_with("SIGN_IN_WAIT"));
    assert!(!ran, "during a wait the password isn't even checked");
}
