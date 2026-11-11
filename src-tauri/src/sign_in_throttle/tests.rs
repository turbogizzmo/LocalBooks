use super::*;

const FILE: &str = "key-1";

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds)
}

fn fail(t: &Throttle, who: &str, times: u32, now: SystemTime) {
    for _ in 0..times {
        t.failed(FILE, Who::User(who), now);
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
    assert!(t.check(FILE, Who::User("ann"), at(0)).is_ok(), "four wrong tries cost nothing");
    fail(&t, "ann", 1, at(0));
    let refused = t.check(FILE, Who::User("ann"), at(10)).unwrap_err();
    assert!(refused.starts_with("SIGN_IN_WAIT") && refused.contains("20 seconds"), "{refused}");
    assert!(t.check(FILE, Who::User("ann"), at(30)).is_ok(), "after the wait, anyone may try again");

    fail(&t, "ann", 10, at(100));
    let refused = t.check(FILE, Who::User("ann"), at(100)).unwrap_err();
    assert!(refused.contains("5 minutes"), "{refused}");
    assert!(t.check(FILE, Who::User("ann"), at(100 + 300)).is_ok(), "never longer than five minutes");
}

#[test]
fn counts_are_per_user_and_per_file_and_a_success_clears_one() {
    let t = Throttle::new(None);
    fail(&t, "Ann", FREE_TRIES + 1, at(0));
    assert!(t.check(FILE, Who::User(" ann "), at(1)).is_err(), "names compare without case or spaces, like the users table");
    assert!(t.check(FILE, Who::User("bob"), at(1)).is_ok(), "another user (say, the other Admin) can still sign in");
    assert!(t.check("key-2", Who::User("ann"), at(1)).is_ok(), "another company file has its own count");
    assert!(t.check(FILE, Who::RecoveryKey, at(1)).is_ok(), "the recovery key has its own count");
    t.succeeded(FILE, Who::User("ANN"));
    assert!(t.check(FILE, Who::User("ann"), at(1)).is_ok());
    fail(&t, "ann", FREE_TRIES, at(2));
    assert!(t.check(FILE, Who::User("ann"), at(2)).is_ok(), "the count started again from zero");
}

#[test]
fn old_counts_expire_and_a_clock_set_back_doesnt_lengthen_the_wait() {
    let t = Throttle::new(None);
    fail(&t, "ann", FREE_TRIES + 4, at(0));
    fail(&t, "bob", 1, at(2 * 24 * 60 * 60));
    assert!(!t.counts.lock().unwrap().contains_key(&Throttle::key(FILE, Who::User("ann"))), "a day without failures forgets the count");

    fail(&t, "cy", FREE_TRIES + 4, at(1000));
    assert!(t.check(FILE, Who::User("cy"), at(10)).is_ok(), "a clock moved back never strands anyone");
}

#[test]
fn counts_survive_a_restart_and_an_unwritable_folder_never_blocks() {
    let dir = std::env::temp_dir().join(format!("localbooks-sign-in-waits-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let first = Throttle::new(Some(&dir));
    fail(&first, "ann", FREE_TRIES + 1, SystemTime::now());
    first.failed(FILE, Who::User("my secret password"), SystemTime::now());
    let saved = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
    assert!(!saved.contains("ann") && !saved.contains("secret") && !saved.contains(FILE), "names and file ids are saved only as hashes: {saved}");
    let after_restart = Throttle::new(Some(&dir));
    assert!(after_restart.check(FILE, Who::User("ann"), SystemTime::now()).is_err(), "restarting Tillhouse doesn't reset the wait");
    let _ = std::fs::remove_dir_all(&dir);

    // A settings "folder" that is really a file: nothing can be saved, the count still works.
    let blocked = std::env::temp_dir().join(format!("localbooks-sign-in-waits-file-{}", std::process::id()));
    std::fs::write(&blocked, b"not a folder").unwrap();
    let t = Throttle::new(Some(&blocked));
    fail(&t, "ann", FREE_TRIES + 1, SystemTime::now());
    assert!(t.check(FILE, Who::User("ann"), SystemTime::now()).is_err());
    let _ = std::fs::remove_file(&blocked);
}

#[tokio::test]
async fn attempts_are_refused_counted_and_cleared() {
    let t = Throttle::new(None);
    let wrong = || async { Err::<(), String>("SIGN_IN_FAILED: the user name or password is not correct".into()) };
    let rule = || async { Err::<(), String>("PASSWORD_TOO_SHORT: use at least 8 characters".into()) };
    let counts = |e: &str| e.starts_with("SIGN_IN_FAILED");
    for _ in 0..FREE_TRIES {
        assert!(t.attempt(FILE, Who::User("ann"), counts, wrong()).await.unwrap_err().starts_with("SIGN_IN_FAILED"));
    }
    for _ in 0..10 {
        assert!(t.attempt(FILE, Who::User("ann"), counts, rule()).await.unwrap_err().starts_with("PASSWORD_TOO_SHORT"), "errors that aren't guesses don't count");
    }
    assert!(t.attempt(FILE, Who::User("ann"), counts, async { Ok(()) }).await.is_ok(), "still within the free tries");
    for _ in 0..=FREE_TRIES {
        let _ = t.attempt(FILE, Who::User("ann"), counts, wrong()).await;
    }
    let mut ran = false;
    let refused = t
        .attempt(FILE, Who::User("ann"), counts, async {
            ran = true;
            Ok(())
        })
        .await;
    assert!(refused.unwrap_err().starts_with("SIGN_IN_WAIT"));
    assert!(!ran, "during a wait the password isn't even checked");
}

#[test]
fn a_user_named_like_the_recovery_key_keeps_its_own_count() {
    let t = Throttle::new(None);
    for _ in 0..FREE_TRIES + 1 {
        t.failed(FILE, Who::RecoveryKey, at(0));
    }
    assert!(t.check(FILE, Who::RecoveryKey, at(1)).is_err());
    assert!(t.check(FILE, Who::User("(recovery key)"), at(1)).is_ok());
    assert!(t.check(FILE, Who::User("recovery"), at(1)).is_ok());
}

#[test]
fn only_so_many_counts_are_kept() {
    let t = Throttle::new(None);
    for i in 0..(MAX_ENTRIES + 50) {
        t.failed(FILE, Who::User(&format!("guess{i}")), at(u64::try_from(i).unwrap()));
    }
    assert!(t.counts.lock().unwrap().len() <= MAX_ENTRIES);
    assert!(t.counts.lock().unwrap().contains_key(&Throttle::key(FILE, Who::User(&format!("guess{}", MAX_ENTRIES + 49)))), "the newest stay");
}

#[tokio::test]
async fn simultaneous_tries_cant_all_pass_the_check() {
    let t = Throttle::new(None);
    let counts = |e: &str| e.starts_with("SIGN_IN_FAILED");
    let slow_wrong = || async {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Err::<(), String>("SIGN_IN_FAILED: wrong".into())
    };
    let tries: Vec<_> = (0..20).map(|_| t.attempt(FILE, Who::User("ann"), counts, slow_wrong())).collect();
    let results = futures_join_all(tries).await;
    let checked = results.iter().filter(|r| r.as_ref().unwrap_err().starts_with("SIGN_IN_FAILED")).count();
    assert_eq!(checked, usize::try_from(FREE_TRIES + 1).unwrap(), "only the free tries and the one that starts the wait were checked");
}

/// Runs the futures together on this task, like simultaneous commands: every one is started (its
/// check and count run before its first await) before any finishes.
async fn futures_join_all<F: std::future::Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut pinned: Vec<_> = futures.into_iter().map(Box::pin).collect();
    let mut done: Vec<Option<F::Output>> = pinned.iter().map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut all = true;
        for (f, out) in pinned.iter_mut().zip(done.iter_mut()) {
            if out.is_none() {
                match f.as_mut().poll(cx) {
                    std::task::Poll::Ready(v) => *out = Some(v),
                    std::task::Poll::Pending => all = false,
                }
            }
        }
        if all { std::task::Poll::Ready(()) } else { std::task::Poll::Pending }
    })
    .await;
    done.into_iter().map(|o| o.expect("finished")).collect()
}
