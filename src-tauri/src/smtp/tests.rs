use super::*;
use crate::db::test_pool;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn store(name: &str) -> SecretStore {
    let path = std::env::temp_dir().join(format!("localbooks-smtp-{name}-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    SecretStore::in_file(path, KEYCHAIN_SERVICE, "the email password")
}

fn account() -> SmtpSettings {
    SmtpSettings { enabled: true, host: "Smtp.Example.com".into(), username: "billing@example.com".into(), from_email: "billing@example.com".into(), from_name: "Acme  Billing".into(), ..SmtpSettings::default() }
}

#[test]
fn settings_are_checked() {
    let s = account().validated().unwrap();
    assert_eq!((s.host.as_str(), s.from_name.as_str()), ("smtp.example.com", "Acme Billing"));
    assert!(SmtpSettings { security: Security::None, ..account() }.validated().is_err(), "a password over a plain connection is refused");
    assert!(SmtpSettings { security: Security::None, username: String::new(), ..account() }.validated().is_ok());
    assert!(SmtpSettings { host: "smtp example.com".into(), ..account() }.validated().is_err());
    assert!(SmtpSettings { from_email: "a@x.com, b@y.com".into(), ..account() }.validated().is_err());
    assert!(SmtpSettings { host: String::new(), ..account() }.validated().is_err(), "sending on needs a server");
    assert!(SmtpSettings { enabled: false, host: String::new(), ..account() }.validated().is_ok());
}

#[tokio::test]
async fn passwords_stay_out_of_the_company_file() {
    let pool = test_pool().await;
    let secrets = store("save");
    assert!(!status(&pool, &secrets).await.unwrap().settings.enabled);

    let saved = save_settings(&pool, &secrets, account(), Some("hunter2".into())).await.unwrap();
    assert!(saved.has_password);
    let stored: String = sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = 'smtp_settings'").fetch_one(&pool).await.unwrap();
    assert!(!stored.contains("hunter2"));
    // The generic preference API cannot read or overwrite it.
    assert!(!crate::preferences::get_all(&pool).await.unwrap().contains_key(SETTINGS_KEY));
    assert!(crate::preferences::set(&pool, SETTINGS_KEY, "{}").await.is_err());

    // A new port keeps the password; clearing it removes it.
    let moved = save_settings(&pool, &secrets, SmtpSettings { port: 2525, ..account() }, None).await.unwrap();
    assert!(moved.has_password);
    // A different account does not inherit the password.
    let other = save_settings(&pool, &secrets, SmtpSettings { port: 2525, username: "someone@example.com".into(), ..account() }, None).await.unwrap();
    assert!(!other.has_password);
    save_settings(&pool, &secrets, SmtpSettings { port: 2525, ..account() }, Some("hunter2".into())).await.unwrap();
    let cleared = save_settings(&pool, &secrets, SmtpSettings { port: 2525, ..account() }, Some(String::new())).await.unwrap();
    assert!(!cleared.has_password);
    let err = send(&pool, &secrets, Outgoing { to: vec!["a@x.com".into()], subject: "s".into(), body: "b".into(), file_name: "f.pdf".into(), pdf: vec![] }).await.unwrap_err();
    assert!(err.starts_with("SMTP_NO_PASSWORD"), "{err}");
}

/// A minimal SMTP server that accepts one message and keeps its text.
async fn mock_server() -> (u16, Arc<Mutex<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(Mutex::new(String::new()));
    let sink = received.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let sink = sink.clone();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut lines = BufReader::new(read).lines();
                write.write_all(b"220 mock ESMTP\r\n").await.unwrap();
                let mut in_data = false;
                while let Ok(Some(line)) = lines.next_line().await {
                    if in_data {
                        if line == "." {
                            in_data = false;
                            write.write_all(b"250 queued\r\n").await.unwrap();
                        } else {
                            let mut text = sink.lock().unwrap();
                            text.push_str(&line);
                            text.push('\n');
                        }
                        continue;
                    }
                    let reply: &[u8] = match line.get(..4).unwrap_or("").to_ascii_uppercase().as_str() {
                        "EHLO" => b"250-mock\r\n250 8BITMIME\r\n",
                        "MAIL" | "RCPT" | "RSET" | "NOOP" => b"250 ok\r\n",
                        "DATA" => {
                            in_data = true;
                            b"354 go ahead\r\n"
                        }
                        "QUIT" => {
                            let _ = write.write_all(b"221 bye\r\n").await;
                            break;
                        }
                        _ => b"502 unsupported\r\n",
                    };
                    write.write_all(reply).await.unwrap();
                }
            });
        }
    });
    (port, received)
}

#[tokio::test]
async fn sends_the_pdf_through_the_server() {
    let (port, received) = mock_server().await;
    let pool = test_pool().await;
    let secrets = store("send");
    let relay = SmtpSettings { host: "127.0.0.1".into(), port, security: Security::None, username: String::new(), ..account() };
    save_settings(&pool, &secrets, relay.clone(), None).await.unwrap();
    test_connection(relay.clone(), &secrets, None).await.unwrap();
    assert_eq!(send_test(relay, &secrets, None, "Acme").await.unwrap(), "billing@example.com");
    assert!(received.lock().unwrap().contains("Subject: LocalBooks test email from Acme"));
    received.lock().unwrap().clear();

    send(
        &pool,
        &secrets,
        Outgoing { to: vec!["ap@initech.test".into()], subject: "Invoice 1001 from Acme".into(), body: "Please find attached.".into(), file_name: "Invoice 1001.pdf".into(), pdf: b"%PDF-1.4 test".to_vec() },
    )
    .await
    .unwrap();
    let text = received.lock().unwrap().clone();
    assert!(text.contains("Subject: Invoice 1001 from Acme"), "{text}");
    assert!(text.contains("From: \"Acme Billing\" <billing@example.com>") || text.contains("From: Acme Billing <billing@example.com>"), "{text}");
    assert!(text.contains("To: ap@initech.test"), "{text}");
    assert!(!text.contains("Bcc:"), "a blind copy is not shown in the message");
    assert!(text.contains("Please find attached."));
    assert!(text.contains("filename=\"Invoice 1001.pdf\""), "{text}");
    assert!(text.contains("Content-Type: application/pdf"));

    // Turned off, nothing is sent.
    save_settings(&pool, &secrets, SmtpSettings { enabled: false, host: "127.0.0.1".into(), port, security: Security::None, username: String::new(), ..account() }, None).await.unwrap();
    let off = send(&pool, &secrets, Outgoing { to: vec!["a@x.com".into()], subject: "s".into(), body: "b".into(), file_name: "f.pdf".into(), pdf: vec![] }).await.unwrap_err();
    assert!(off.starts_with("SMTP_OFF"));
}
