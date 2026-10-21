//! One LocalBooks per user: a second launch hands its request to the running app and exits.
//!
//! The running app listens on a random loopback port and writes that port and a fresh random
//! secret to `instance.json` in the per-user app config folder (owner-only permissions on Unix).
//! A new launch reads the file, connects, and sends `{secret, action, path}` as one JSON line:
//! `open` (a double-clicked `.localbooks` file) or `focus` (a plain launch). The running app
//! checks the secret, replies, then opens the file through the same validation as File › Open and
//! brings its window forward. If nothing answers (no app running, or a stale file after a crash),
//! the new launch simply starts normally.
//!
//! This channel is separate from the MCP server on purpose: that server is off by default and
//! strictly read-only, while this one changes which company file is open.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const INSTANCE_FILE: &str = "instance.json";
const MAX_LINE_BYTES: u64 = 16 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(800);
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstanceFile {
    port: u16,
    secret: String,
    pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum HandOff {
    /// Open this company file in the running app.
    Open { path: PathBuf },
    /// Just bring the running app's window forward.
    Focus,
}

#[derive(Serialize, Deserialize)]
struct Message {
    secret: String,
    #[serde(flatten)]
    request: HandOff,
}

#[derive(Serialize, Deserialize)]
struct Reply {
    ok: bool,
}

fn instance_path(config_dir: &Path) -> PathBuf {
    config_dir.join(INSTANCE_FILE)
}

/// Writes `bytes` to `path` (atomically) readable by its owner only.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("json.partial-{}", std::process::id()));
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Called at launch, before anything else opens: returns true when a running LocalBooks accepted
/// the request, in which case this process should exit.
pub fn hand_off(config_dir: &Path, request: &HandOff) -> bool {
    let Some(file) = std::fs::read(instance_path(config_dir)).ok().and_then(|b| serde_json::from_slice::<InstanceFile>(&b).ok()) else {
        return false;
    };
    let try_send = || -> std::io::Result<bool> {
        let mut stream = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], file.port)), CONNECT_TIMEOUT)?;
        stream.set_read_timeout(Some(REPLY_TIMEOUT))?;
        stream.set_write_timeout(Some(REPLY_TIMEOUT))?;
        let mut line = serde_json::to_vec(&Message { secret: file.secret.clone(), request: request.clone() })?;
        line.push(b'\n');
        stream.write_all(&line)?;
        let mut reply = String::new();
        BufReader::new(stream.take(1024)).read_line(&mut reply)?;
        Ok(serde_json::from_str::<Reply>(&reply).is_ok_and(|r| r.ok))
    };
    try_send().unwrap_or(false)
}

/// The request a launch should hand off: the `.localbooks` file it was given (made absolute, since
/// the running app has a different working directory), or a plain focus request.
pub fn request_for(launch_file: Option<&Path>) -> HandOff {
    match launch_file {
        Some(path) => {
            let absolute = path.canonicalize().unwrap_or_else(|_| std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf()));
            HandOff::Open { path: absolute }
        }
        None => HandOff::Focus,
    }
}

/// Removes the instance file when the running app exits (if it is still this app's file).
pub struct InstanceGuard {
    path: PathBuf,
    secret: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        self.task.abort();
        let ours = std::fs::read(&self.path).ok().and_then(|b| serde_json::from_slice::<InstanceFile>(&b).ok()).is_some_and(|f| f.secret == self.secret);
        if ours {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Starts accepting hand-offs from later launches; `handle` runs for each accepted request.
pub async fn listen<F>(config_dir: &Path, handle: F) -> Result<InstanceGuard, String>
where
    F: Fn(HandOff) + Send + Sync + 'static,
{
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await.map_err(|e| format!("INSTANCE_BIND: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("INSTANCE_BIND: {e}"))?.port();
    let secret = crate::mcp_server::new_token();
    std::fs::create_dir_all(config_dir).map_err(|e| format!("INSTANCE_FILE: {e}"))?;
    let path = instance_path(config_dir);
    let file = serde_json::to_vec(&InstanceFile { port, secret: secret.clone(), pid: std::process::id() }).map_err(|e| e.to_string())?;
    write_private(&path, &file).map_err(|e| format!("INSTANCE_FILE: cannot write {}: {e}", path.display()))?;

    let expected = secret.clone();
    let handle = std::sync::Arc::new(handle);
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else { continue };
            if !peer.ip().is_loopback() {
                continue;
            }
            let (expected, handle) = (expected.clone(), handle.clone());
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut line = String::new();
                let mut reader = tokio::io::BufReader::new(tokio::io::AsyncReadExt::take(read, MAX_LINE_BYTES));
                let read = tokio::time::timeout(REPLY_TIMEOUT, reader.read_line(&mut line)).await;
                let message = match read {
                    Ok(Ok(_)) => serde_json::from_str::<Message>(&line).ok(),
                    _ => None,
                };
                let accepted = message.filter(|m| crate::mcp_server::tokens_match(&m.secret, &expected));
                let reply = serde_json::to_vec(&Reply { ok: accepted.is_some() }).unwrap_or_default();
                let _ = write.write_all(&reply).await;
                let _ = write.write_all(b"\n").await;
                let _ = write.shutdown().await;
                if let Some(message) = accepted {
                    handle(message.request);
                }
            });
        }
    });
    Ok(InstanceGuard { path, secret, task })
}

#[cfg(test)]
mod tests {
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
}
