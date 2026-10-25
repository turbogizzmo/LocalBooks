//! Hands a form to the user's own mail app as a new message with the PDF attached.
//!
//! Nothing is sent from LocalBooks: the message opens in the mail app for the user to review and
//! send. How the attachment gets there depends on the platform:
//!
//! - macOS: Apple Mail and Microsoft Outlook (classic) are scripted through Apple Events
//!   (`osascript`, with the message text passed as arguments, never spliced into the script).
//! - Windows: Simple MAPI (`MAPISendMailW`), which Outlook, Thunderbird and other MAPI clients
//!   implement.
//! - Linux: `xdg-email --attach`.
//!
//! When the default mail app cannot take an attachment that way, a `mailto:` link opens the
//! message (recipient, subject and text) and the PDF is shown in the file manager to attach.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// A message to open in the mail app.
#[derive(Debug, Clone)]
pub struct Compose {
    /// Addresses, already validated.
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub attachment: PathBuf,
}

/// What happened, for the confirmation shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ComposeOutcome {
    /// A new message is open in `app` with the PDF attached.
    Attached { app: String },
    /// The message opened from a `mailto:` link and the PDF is shown in the file manager; the user
    /// attaches it.
    AttachManually { path: String },
}

// ------------------------------------------------------------------------------------------------
// Addresses and mailto links
// ------------------------------------------------------------------------------------------------

/// Splits "a@x.com, b@y.org" into addresses, refusing anything that is not a plain address.
pub fn parse_recipients(text: &str) -> Result<Vec<String>, String> {
    let addresses: Vec<String> = text.split([',', ';']).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
    if addresses.is_empty() {
        return Err("MAIL_NO_RECIPIENT: enter the customer's email address".into());
    }
    if addresses.len() > 20 {
        return Err("MAIL_TOO_MANY_RECIPIENTS: send to at most 20 addresses at once".into());
    }
    for address in &addresses {
        let valid = address.len() <= 254
            && !address.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '(' | ')' | '[' | ']' | '\\'))
            && match address.rsplit_once('@') {
                Some((local, domain)) => !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.') && !domain.contains('@'),
                None => false,
            };
        if !valid {
            return Err(format!("MAIL_BAD_ADDRESS: '{address}' is not an email address"));
        }
    }
    Ok(addresses)
}

/// RFC 3986 percent-encoding of everything but unreserved characters.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A `mailto:` link (RFC 6068) with the recipients, subject and body; line breaks become CRLF.
pub fn mailto_url(message: &Compose) -> String {
    let to: Vec<String> = message.to.iter().map(|a| percent_encode(a).replace("%40", "@")).collect();
    let body = message.body.replace("\r\n", "\n").replace('\n', "\r\n");
    format!("mailto:{}?subject={}&body={}", to.join(","), percent_encode(&message.subject), percent_encode(&body))
}

// ------------------------------------------------------------------------------------------------
// Platforms
// ------------------------------------------------------------------------------------------------

/// Opens a new message in the user's mail app. Blocking: run it off the async workers.
pub fn compose(message: &Compose) -> Result<ComposeOutcome, String> {
    match platform::attach(message) {
        Ok(app) => Ok(ComposeOutcome::Attached { app }),
        Err(reason) => {
            eprintln!("mail app attachment unavailable, using a mailto link: {reason}");
            platform::open_url(&mailto_url(message))?;
            platform::reveal(&message.attachment)?;
            Ok(ComposeOutcome::AttachManually { path: message.attachment.display().to_string() })
        }
    }
}

fn run(command: &mut std::process::Command) -> Result<String, String> {
    let output = command.output().map_err(|e| format!("cannot start {:?}: {e}", command.get_program()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!("{:?} failed: {}", command.get_program(), String::from_utf8_lossy(&output.stderr).trim()))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{run, Compose};
    use std::path::Path;
    use std::process::Command;

    /// Prints the bundle id of the app that opens mailto: links.
    const DEFAULT_MAIL_APP: &str = "ObjC.import('AppKit'); \
        var url = $.NSWorkspace.sharedWorkspace.URLForApplicationToOpenURL($.NSURL.URLWithString('mailto:someone@example.com')); \
        url.isNil() ? '' : ObjC.unwrap($.NSBundle.bundleWithURL(url).bundleIdentifier);";

    // Arguments: subject, body, attachment path, then one argument per recipient.
    const APPLE_MAIL: &str = r#"on run argv
    set theSubject to item 1 of argv
    set theBody to item 2 of argv
    set thePath to item 3 of argv
    tell application "Mail"
        set msg to make new outgoing message with properties {subject:theSubject, content:theBody & return & return, visible:true}
        tell msg
            repeat with i from 4 to count of argv
                make new to recipient at end of to recipients with properties {address:(item i of argv)}
            end repeat
            make new attachment with properties {file name:(POSIX file thePath)} at after the last paragraph
        end tell
        activate
    end tell
end run"#;

    const OUTLOOK: &str = r#"on run argv
    set theSubject to item 1 of argv
    set theBody to item 2 of argv
    set thePath to item 3 of argv
    tell application "Microsoft Outlook"
        set msg to make new outgoing message with properties {subject:theSubject, plain text content:theBody}
        repeat with i from 4 to count of argv
            make new to recipient at msg with properties {email address:{address:(item i of argv)}}
        end repeat
        make new attachment at msg with properties {file:(POSIX file thePath)}
        open msg
        activate
    end tell
end run"#;

    pub fn attach(message: &Compose) -> Result<String, String> {
        let handler = run(Command::new("osascript").args(["-l", "JavaScript", "-e", DEFAULT_MAIL_APP]))?;
        let (script, name) = match handler.to_ascii_lowercase().as_str() {
            "com.apple.mail" => (APPLE_MAIL, "Mail"),
            "com.microsoft.outlook" => (OUTLOOK, "Microsoft Outlook"),
            other => return Err(format!("the default mail app ({}) is not scriptable", if other.is_empty() { "none" } else { other })),
        };
        let mut command = Command::new("osascript");
        // "--" ends osascript's options, so a subject such as "-e" is passed through as text.
        command.args(["-e", script, "--", &message.subject, &message.body]).arg(&message.attachment).args(&message.to);
        run(&mut command)?;
        Ok(name.to_string())
    }

    pub fn open_url(url: &str) -> Result<(), String> {
        run(Command::new("open").arg(url)).map(|_| ()).map_err(|e| format!("MAIL_OPEN: cannot open the mail app: {e}"))
    }

    pub fn reveal(path: &Path) -> Result<(), String> {
        run(Command::new("open").arg("-R").arg(path)).map(|_| ()).map_err(|e| format!("MAIL_REVEAL: {e}"))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{run, Compose};
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::FreeLibrary;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::System::Mapi::{MapiFileDescW, MapiMessageW, MapiRecipDescW, MAPI_DIALOG, MAPI_LOGON_UI, MAPI_TO, MAPI_USER_ABORT, SUCCESS_SUCCESS};

    type SendMailW = unsafe extern "system" fn(session: usize, ui_param: usize, message: *const MapiMessageW, flags: u32, reserved: u32) -> u32;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn send_mail(message: &Compose) -> Result<(), String> {
        let mut subject = wide(&message.subject);
        let mut body = wide(&message.body.replace("\r\n", "\n").replace('\n', "\r\n"));
        let mut path = wide(&message.attachment.display().to_string());
        let mut file_name = wide(&message.attachment.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let mut addresses: Vec<Vec<u16>> = message.to.iter().map(|a| wide(&format!("SMTP:{a}"))).collect();
        let mut names: Vec<Vec<u16>> = message.to.iter().map(|a| wide(a)).collect();
        let mut recipients: Vec<MapiRecipDescW> = addresses
            .iter_mut()
            .zip(names.iter_mut())
            .map(|(address, name)| MapiRecipDescW { ulRecipClass: MAPI_TO, lpszName: name.as_mut_ptr(), lpszAddress: address.as_mut_ptr(), ..Default::default() })
            .collect();
        let mut file = MapiFileDescW { nPosition: u32::MAX, lpszPathName: path.as_mut_ptr(), lpszFileName: file_name.as_mut_ptr(), ..Default::default() };
        let mapi = MapiMessageW {
            lpszSubject: subject.as_mut_ptr(),
            lpszNoteText: body.as_mut_ptr(),
            nRecipCount: recipients.len() as u32,
            lpRecips: recipients.as_mut_ptr(),
            nFileCount: 1,
            lpFiles: &mut file,
            ..Default::default()
        };
        // MAPISendMailW is exported by the mapi32.dll stub (which forwards to the default mail
        // app) but not by the SDK's import library, so it is looked up at run time.
        let dll = wide("mapi32.dll");
        // SAFETY: a NUL-terminated name; the module is freed below.
        let module = unsafe { LoadLibraryW(dll.as_ptr()) };
        if module.is_null() {
            return Err("mapi32.dll is not available".into());
        }
        // SAFETY: a NUL-terminated ASCII name in a loaded module.
        let Some(proc) = (unsafe { GetProcAddress(module, c"MAPISendMailW".as_ptr().cast()) }) else {
            // SAFETY: loaded above and not used after this.
            unsafe { FreeLibrary(module) };
            return Err("the mail app does not offer MAPISendMailW".into());
        };
        // SAFETY: MAPISendMailW has this signature (mapi.h); every pointer in `mapi` refers to a
        // buffer above that outlives the call, after which the module is freed.
        let code = unsafe {
            let send: SendMailW = std::mem::transmute(proc);
            let code = send(0, 0, &mapi, MAPI_DIALOG | MAPI_LOGON_UI, 0);
            FreeLibrary(module);
            code
        };
        match code {
            SUCCESS_SUCCESS | MAPI_USER_ABORT => Ok(()),
            other => Err(format!("MAPISendMailW returned {other}")),
        }
    }

    pub fn attach(message: &Compose) -> Result<String, String> {
        // MAPI clients may keep the call open until their compose window closes. A failure shows
        // up quickly; a call still running after a few seconds means the window is open.
        let (done, result) = std::sync::mpsc::channel();
        let message = message.clone();
        std::thread::spawn(move || {
            let _ = done.send(send_mail(&message));
        });
        match result.recv_timeout(Duration::from_secs(4)) {
            Ok(Ok(())) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok("your mail app".to_string()),
            Ok(Err(e)) => Err(e),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err("the mail app request stopped".into()),
        }
    }

    pub fn open_url(url: &str) -> Result<(), String> {
        // The shell's URL handler opens the mailto: link without a command interpreter parsing it.
        run(Command::new("rundll32").args(["url.dll,FileProtocolHandler", url])).map(|_| ()).map_err(|e| format!("MAIL_OPEN: cannot open the mail app: {e}"))
    }

    pub fn reveal(path: &Path) -> Result<(), String> {
        // Explorer exits with status 1 even when it opens the window.
        Command::new("explorer").arg(format!("/select,{}", path.display())).spawn().map(|_| ()).map_err(|e| format!("MAIL_REVEAL: {e}"))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use super::{run, Compose};
    use std::path::Path;
    use std::process::Command;

    pub fn attach(message: &Compose) -> Result<String, String> {
        let mut command = Command::new("xdg-email");
        command.args(["--subject", &message.subject, "--body", &message.body, "--attach"]).arg(&message.attachment).args(&message.to);
        run(&mut command)?;
        Ok("your mail app".to_string())
    }

    pub fn open_url(url: &str) -> Result<(), String> {
        run(Command::new("xdg-open").arg(url)).map(|_| ()).map_err(|e| format!("MAIL_OPEN: cannot open the mail app: {e}"))
    }

    pub fn reveal(path: &Path) -> Result<(), String> {
        let folder = path.parent().unwrap_or(path);
        run(Command::new("xdg-open").arg(folder)).map(|_| ()).map_err(|e| format!("MAIL_REVEAL: {e}"))
    }
}

/// Writes the PDF to a fresh folder under `outgoing_dir` (so two sends never share a file) and
/// returns its path. Older folders are removed: once a message is sent the mail app has its own
/// copy, and these are only ever regenerated.
pub fn stage_attachment(outgoing_dir: &Path, file_name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    if let Ok(entries) = std::fs::read_dir(outgoing_dir) {
        let day_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(24 * 3600);
        for entry in entries.flatten() {
            let stale = entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < day_ago);
            if stale {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    let dir = outgoing_dir.join(stamp.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("MAIL_STAGE: cannot create {}: {e}", dir.display()))?;
    let path = dir.join(file_name);
    std::fs::write(&path, bytes).map_err(|e| format!("MAIL_STAGE: cannot write {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests;
