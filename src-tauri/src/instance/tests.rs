use super::*;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("localbooks-instance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn hand_off_async(dir: &Path, request: HandOff) -> bool {
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || hand_off(&dir, &request)).await.unwrap()
}

#[tokio::test]
async fn a_second_launch_hands_its_file_to_the_running_app() {
    let tmp = TempDir::new("handoff");
    assert!(!hand_off_async(&tmp.0, HandOff::Focus).await, "nothing running: start normally");

    let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
    let guard = listen(&tmp.0, move |request| {
        let _ = sender.send(request);
    })
    .await
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(tmp.0.join(INSTANCE_FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "only the owner can read the secret");
    }

    let file = PathBuf::from("/books/Acme Ltd.localbooks");
    assert!(hand_off_async(&tmp.0, HandOff::Open { path: file.clone() }).await);
    assert_eq!(received.recv().await.unwrap(), HandOff::Open { path: file });
    assert!(hand_off_async(&tmp.0, HandOff::Focus).await);
    assert_eq!(received.recv().await.unwrap(), HandOff::Focus);

    // A caller without the secret is refused and nothing happens.
    let info: InstanceFile = serde_json::from_slice(&std::fs::read(tmp.0.join(INSTANCE_FILE)).unwrap()).unwrap();
    let forged = InstanceFile { secret: "0".repeat(64), ..info.clone() };
    std::fs::write(tmp.0.join(INSTANCE_FILE), serde_json::to_vec(&forged).unwrap()).unwrap();
    assert!(!hand_off_async(&tmp.0, HandOff::Focus).await);
    assert!(tokio::time::timeout(Duration::from_millis(200), received.recv()).await.is_err());
    std::fs::write(tmp.0.join(INSTANCE_FILE), serde_json::to_vec(&info).unwrap()).unwrap();

    // Quitting removes the file and closes the port; a stale file just means "start normally".
    drop(guard);
    assert!(!tmp.0.join(INSTANCE_FILE).exists());
    std::fs::write(tmp.0.join(INSTANCE_FILE), serde_json::to_vec(&info).unwrap()).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!hand_off_async(&tmp.0, HandOff::Focus).await);
}

#[test]
fn launch_requests_use_absolute_paths() {
    assert_eq!(request_for(None), HandOff::Focus);
    let HandOff::Open { path } = request_for(Some(Path::new("relative/Books.localbooks"))) else { panic!("open request") };
    assert!(path.is_absolute() && path.ends_with("relative/Books.localbooks"));
}
