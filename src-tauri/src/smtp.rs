//! Optional sending of forms over SMTP, from the company's own mail account.
//!
//! Off by default: forms open in the user's mail app (see `mail_app`). When turned on, the email
//! dialog sends the message from LocalBooks through the configured server. The server settings
//! live in the company file (`smtp_settings` preference); the password stays on this computer, in
//! the operating system's credential store, never in the company file or its backups.
//!
//! Connections use STARTTLS (port 587) or implicit TLS (port 465). A plain connection is allowed
//! only without a username, for a local relay, so a password never crosses the network unencrypted.

use std::path::Path;
use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::secrets::SecretStore;

pub const SETTINGS_KEY: &str = "smtp_settings";
const KEYCHAIN_SERVICE: &str = "LocalBooks SMTP";
const SECRETS_FILE: &str = "smtp-secrets.json";
const TIMEOUT: Duration = Duration::from_secs(30);

pub fn secret_store(config_dir: &Path) -> SecretStore {
    SecretStore::for_this_computer(config_dir, KEYCHAIN_SERVICE, SECRETS_FILE, "the email password")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Security {
    /// Plain connection upgraded with STARTTLS (normally port 587).
    #[default]
    Starttls,
    /// TLS from the start (normally port 465).
    Tls,
    /// No encryption: only for a local relay without a password.
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SmtpSettings {
    /// Send emailed forms through this server instead of opening the mail app.
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub security: Security,
    /// Empty for a server that needs no sign-in.
    pub username: String,
    pub from_email: String,
    pub from_name: String,
    /// Also send a copy to the from address, so the sent message is on record.
    pub copy_to_self: bool,
}

impl Default for SmtpSettings {
    fn default() -> Self {
        SmtpSettings { enabled: false, host: String::new(), port: 587, security: Security::Starttls, username: String::new(), from_email: String::new(), from_name: String::new(), copy_to_self: true }
    }
}

impl SmtpSettings {
    /// Trims and checks the settings; a disabled configuration may be incomplete.
    pub fn validated(mut self) -> Result<Self, String> {
        self.host = self.host.trim().trim_end_matches('.').to_ascii_lowercase();
        self.username = self.username.trim().to_string();
        self.from_email = self.from_email.trim().to_string();
        self.from_name = self.from_name.split_whitespace().collect::<Vec<_>>().join(" ");
        if self.host.len() > 253 || self.host.chars().any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))) {
            return Err("SMTP_BAD_HOST: enter the server name, such as smtp.example.com".into());
        }
        if self.username.chars().count() > 200 || self.from_name.chars().count() > 100 {
            return Err("SMTP_TOO_LONG: the username or sender name is too long".into());
        }
        if !self.from_email.is_empty() {
            crate::mail_app::parse_recipients(&self.from_email).map_err(|_| "SMTP_BAD_FROM: enter the address messages are sent from".to_string())?;
            if self.from_email.contains([',', ';']) {
                return Err("SMTP_BAD_FROM: enter a single from address".into());
            }
        }
        if self.port == 0 {
            return Err("SMTP_BAD_PORT: enter the server port, usually 587 or 465".into());
        }
        if self.security == Security::None && !self.username.is_empty() {
            return Err("SMTP_INSECURE: a server that needs a password must use STARTTLS or TLS".into());
        }
        if self.enabled && (self.host.is_empty() || self.from_email.is_empty()) {
            return Err("SMTP_INCOMPLETE: enter the server and the from address before turning sending on".into());
        }
        Ok(self)
    }

    /// The credential-store key for this account's password.
    fn secret_key(&self) -> String {
        format!("{}@{}:{}", self.username, self.host, self.port)
    }
}

pub async fn get_settings(pool: &SqlitePool) -> Result<SmtpSettings, String> {
    let stored: Option<String> = sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = ?1").bind(SETTINGS_KEY).fetch_optional(pool).await.map_err(db_err)?;
    Ok(stored.and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default())
}

/// The settings and whether a password is saved for them on this computer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmtpStatus {
    pub settings: SmtpSettings,
    pub has_password: bool,
}

pub async fn status(pool: &SqlitePool, secrets: &SecretStore) -> Result<SmtpStatus, String> {
    let settings = get_settings(pool).await?;
    let has_password = !settings.username.is_empty() && secrets.get(&settings.secret_key()).await?.is_some();
    Ok(SmtpStatus { settings, has_password })
}

/// Saves the settings; `password` replaces the saved one when given (empty removes it).
pub async fn save_settings(pool: &SqlitePool, secrets: &SecretStore, settings: SmtpSettings, password: Option<String>) -> Result<SmtpStatus, String> {
    let settings = settings.validated()?;
    let previous = get_settings(pool).await?;
    if let Some(password) = password {
        if password.is_empty() {
            secrets.delete(&settings.secret_key()).await?;
        } else if !settings.username.is_empty() {
            secrets.put(&settings.secret_key(), &password).await?;
        }
    } else if previous.secret_key() != settings.secret_key() && !settings.username.is_empty() && previous.username == settings.username {
        // Same account, new server name or port: carry the saved password over.
        if let Some(saved) = secrets.get(&previous.secret_key()).await? {
            secrets.put(&settings.secret_key(), &saved).await?;
        }
    }
    if previous.secret_key() != settings.secret_key() {
        secrets.delete(&previous.secret_key()).await?;
    }
    let json = serde_json::to_string(&settings).map_err(|e| format!("SMTP_SAVE: {e}"))?;
    sqlx::query(
        "INSERT INTO system_preferences (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(SETTINGS_KEY)
    .bind(&json)
    .execute(pool)
    .await
    .map_err(db_err)?;
    status(pool, secrets).await
}

/// Builds the transport, using `password` or the saved one.
async fn transport(settings: &SmtpSettings, secrets: &SecretStore, password: Option<String>) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    if settings.host.is_empty() {
        return Err("SMTP_INCOMPLETE: enter the email server in Settings › Email".into());
    }
    let bad_host = |e: lettre::transport::smtp::Error| format!("SMTP_BAD_HOST: {e}");
    let builder = match settings.security {
        Security::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.host).map_err(bad_host)?,
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.host).map_err(bad_host)?,
        Security::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&settings.host),
    };
    let mut builder = builder.port(settings.port).timeout(Some(TIMEOUT));
    if !settings.username.is_empty() {
        let password = match password.filter(|p| !p.is_empty()) {
            Some(p) => p,
            None => secrets.get(&settings.secret_key()).await?.ok_or("SMTP_NO_PASSWORD: enter the email password in Settings › Email (it is kept on this computer only)")?,
        };
        builder = builder.credentials(Credentials::new(settings.username.clone(), password));
    }
    Ok(builder.build())
}

/// Readable SMTP failures: sign-in, refusals and connection problems.
fn send_error(e: &lettre::transport::smtp::Error) -> String {
    let text = e.to_string();
    if e.is_permanent() && text.to_ascii_lowercase().contains("auth") {
        format!("SMTP_AUTH: the server refused the username or password ({text})")
    } else if e.is_permanent() {
        format!("SMTP_REFUSED: the server refused the message ({text})")
    } else if e.is_transient() {
        format!("SMTP_TRY_LATER: the server could not take the message right now ({text})")
    } else {
        format!("SMTP_CONNECT: cannot reach the email server ({text})")
    }
}

fn mailbox(settings: &SmtpSettings) -> Result<Mailbox, String> {
    let address = settings.from_email.parse().map_err(|e| format!("SMTP_BAD_FROM: {e}"))?;
    Ok(Mailbox::new(Some(settings.from_name.clone()).filter(|n| !n.is_empty()), address))
}

/// Checks that LocalBooks can connect and sign in with these (possibly unsaved) settings.
pub async fn test_connection(settings: SmtpSettings, secrets: &SecretStore, password: Option<String>) -> Result<(), String> {
    let settings = settings.validated()?;
    let transport = transport(&settings, secrets, password).await?;
    match transport.test_connection().await {
        Ok(true) => Ok(()),
        Ok(false) => Err("SMTP_CONNECT: the server did not answer".into()),
        Err(e) => Err(send_error(&e)),
    }
}

/// Sends a short message to the from address with these (possibly unsaved) settings; returns
/// the address it went to.
pub async fn send_test(settings: SmtpSettings, secrets: &SecretStore, password: Option<String>, company: &str) -> Result<String, String> {
    let settings = settings.validated()?;
    if settings.from_email.is_empty() {
        return Err("SMTP_INCOMPLETE: enter the from address first".into());
    }
    let transport = transport(&settings, secrets, password).await?;
    let from = mailbox(&settings)?;
    let message = Message::builder()
        .from(from.clone())
        .to(from)
        .subject(format!("LocalBooks test email from {company}"))
        .body(format!("This is a test message from LocalBooks.\n\nIf you can read it, {company} can email invoices and estimates through {}.", settings.host))
        .map_err(|e| format!("SMTP_MESSAGE: {e}"))?;
    transport.send(message).await.map_err(|e| send_error(&e))?;
    Ok(settings.from_email)
}

/// A message with a PDF attached.
pub struct Outgoing {
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub file_name: String,
    pub pdf: Vec<u8>,
}

fn build_message(settings: &SmtpSettings, outgoing: Outgoing) -> Result<Message, String> {
    let from = mailbox(settings)?;
    let mut builder = Message::builder().from(from.clone()).subject(outgoing.subject);
    for to in &outgoing.to {
        builder = builder.to(to.parse().map_err(|e| format!("MAIL_BAD_ADDRESS: '{to}': {e}"))?);
    }
    if settings.copy_to_self {
        builder = builder.bcc(from);
    }
    let pdf = Attachment::new(outgoing.file_name).body(outgoing.pdf, ContentType::parse("application/pdf").expect("a valid media type"));
    builder.multipart(MultiPart::mixed().singlepart(SinglePart::plain(outgoing.body)).singlepart(pdf)).map_err(|e| format!("SMTP_MESSAGE: {e}"))
}

/// Sends the message through the configured server.
pub async fn send(pool: &SqlitePool, secrets: &SecretStore, outgoing: Outgoing) -> Result<(), String> {
    let settings = get_settings(pool).await?;
    if !settings.enabled {
        return Err("SMTP_OFF: sending from LocalBooks is turned off in Settings › Email".into());
    }
    let transport = transport(&settings, secrets, None).await?;
    let message = build_message(&settings, outgoing)?;
    transport.send(message).await.map(|_| ()).map_err(|e| send_error(&e))
}

#[cfg(test)]
mod tests;
