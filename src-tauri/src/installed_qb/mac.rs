//! QuickBooks Desktop for Mac through its AppleScript dictionary (`QuickBooks.sdef`): the
//! `executeqbxml` command runs a qbXML request on the open company, and `reportAsText` returns the
//! report in the front window. Scripts run through `osascript`, with the request passed as an
//! argument, never spliced into the script. The commands are named by their Apple event codes
//! (`QkBkExml`, `QkBkRepo`), which work whichever QuickBooks version is running.
//!
//! The first request makes macOS ask whether LocalBooks may control QuickBooks (the Automation
//! permission); a refusal comes back as error -1743, reported with where to turn it on.

use std::path::PathBuf;
use std::process::Command;

use super::{Status, Transport};

/// Bundle identifiers of QuickBooks for Mac start with this (`com.intuit.QuickBooks2024`).
const BUNDLE_PREFIX: &str = "com.intuit.QuickBooks";

/// Prints `bundle id<TAB>name` for each running QuickBooks. Reading the running apps needs no
/// permission.
const RUNNING: &str = "ObjC.import('AppKit'); \
    var apps = $.NSWorkspace.sharedWorkspace.runningApplications, out = []; \
    for (var i = 0; i < apps.count; i++) { var a = apps.objectAtIndex(i); \
      var id = ObjC.unwrap(a.bundleIdentifier) || ''; \
      if (id.indexOf('com.intuit.QuickBooks') === 0 && a.activationPolicy === 0) out.push(id + '\\t' + ObjC.unwrap(a.localizedName)); } \
    out.join('\\n');";

/// Prints the open company's name, or nothing when no company is open. Argument: the bundle id.
const COMPANY: &str = r#"on run argv
    tell application id (item 1 of argv)
        if (count of every «class docu») is 0 then return ""
        return «property pnam» of «class docu» 1
    end tell
end run"#;

/// Runs a qbXML request. Arguments: the bundle id, the request.
const EXECUTE: &str = r#"on run argv
    set theRequest to item 2 of argv
    with timeout of 1800 seconds
        tell application id (item 1 of argv) to return «event QkBkExml» theRequest
    end timeout
end run"#;

/// Prints the front window's title on the first line, then the report it shows as text.
/// Argument: the bundle id.
const FRONT_REPORT: &str = r#"on run argv
    with timeout of 1800 seconds
        tell application id (item 1 of argv)
            set theTitle to ""
            try
                set theTitle to «property pnam» of «class cwin» 1
            end try
            set theReport to «event QkBkRepo»
        end tell
    end timeout
    return theTitle & linefeed & theReport
end run"#;

pub struct AppleScript {
    bundle_id: String,
    name: String,
}

/// Runs a script, turning macOS' AppleScript errors into what to do about them.
fn osascript(script: &str, language: &str, args: &[&str], app: &str) -> Result<String, String> {
    let output = Command::new("osascript")
        .args(["-l", language, "-e", script, "--"])
        .args(args)
        .output()
        .map_err(|e| format!("QB_SCRIPT_FAILED: cannot run osascript: {e}"))?;
    if output.status.success() {
        // osascript ends its output with a newline of its own.
        let mut out = String::from_utf8_lossy(&output.stdout).into_owned();
        if out.ends_with('\n') {
            out.pop();
        }
        return Ok(out);
    }
    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if err.contains("-1743") {
        format!(
            "QB_AUTOMATION_DENIED: macOS did not let LocalBooks ask {app} for your data. Open System Settings › Privacy & Security › Automation, \
             turn on {app} under LocalBooks, then try again"
        )
    } else if err.contains("-600") {
        format!("QB_NOT_RUNNING: {app} is not running. Open it and your company file, then try again")
    } else if err.contains("-1712") {
        format!("QB_TIMEOUT: {app} did not answer in time. Close any window or message waiting in {app}, then try again")
    } else if err.contains("-1708") || err.contains("-1728") || err.contains("understand") {
        format!(
            "QB_NO_SCRIPTING: this version of {app} does not answer LocalBooks' requests (QuickBooks Desktop for Mac 2024 does). \
             Export the Journal and lists from its Reports menu instead, and choose the files in step 1 ({err})"
        )
    } else {
        format!("QB_SCRIPT_FAILED: {app} could not be asked for your data ({err})")
    })
}

/// QuickBooks apps in the Applications folders, one level down too (`/Applications/QuickBooks 2024/`).
fn installed() -> Vec<String> {
    let mut dirs = vec![(PathBuf::from("/Applications"), 0)];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push((PathBuf::from(home).join("Applications"), 0));
    }
    let mut found = Vec::new();
    while let Some((dir, depth)) = dirs.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("QuickBooks") {
                continue;
            }
            match name.strip_suffix(".app") {
                Some(app) => found.push(app.to_string()),
                None if depth == 0 && e.path().is_dir() => dirs.push((e.path(), 1)),
                None => {}
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The running QuickBooks apps, as (bundle id, name).
fn running() -> Vec<(String, String)> {
    osascript(RUNNING, "JavaScript", &[], "QuickBooks")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(id, _)| id.starts_with(BUNDLE_PREFIX))
        .map(|(id, name)| (id.to_string(), name.to_string()))
        .collect()
}

pub fn status() -> Status {
    Status { platform: Some("mac"), installed: installed(), running: running().into_iter().next().map(|(_, name)| name), sdk_installed: false }
}

/// Finds the running QuickBooks and checks that a company is open in it. This is the first
/// request, so macOS asks for the Automation permission here.
pub fn connect() -> Result<AppleScript, String> {
    let Some((bundle_id, name)) = running().into_iter().next() else {
        return Err(match installed().first() {
            Some(app) => format!("QB_NOT_RUNNING: {app} is not running. Open it and your company file, then try again"),
            None => "QB_NOT_FOUND: QuickBooks Desktop for Mac was not found on this Mac. Open it and your company file, or export its reports instead".into(),
        });
    };
    let company = osascript(COMPANY, "AppleScript", &[&bundle_id], &name)?;
    if company.trim().is_empty() {
        return Err(format!("QB_NO_COMPANY: no company file is open in {name}. Open your company file in {name}, then try again"));
    }
    Ok(AppleScript { bundle_id, name })
}

impl Transport for AppleScript {
    fn execute(&mut self, request: &str) -> Result<String, String> {
        osascript(EXECUTE, "AppleScript", &[&self.bundle_id, request], &self.name)
    }

    fn journal_report(&mut self) -> Result<String, String> {
        let open_it = format!(
            "In {}, choose Reports › Accountant & Taxes › Journal (or Reports › Journal), set the dates to All, keep the Debit and Credit columns, \
             leave the report as the front window, then try again",
            self.name
        );
        let out = osascript(FRONT_REPORT, "AppleScript", &[&self.bundle_id], &self.name)?;
        let (title, report) = out.split_once('\n').unwrap_or((out.as_str(), ""));
        if !title.to_lowercase().contains("journal") {
            let front = if title.is_empty() { "no report".to_string() } else { format!("\"{title}\"") };
            return Err(format!("QB_NO_JOURNAL: the front window in {} shows {front}, not the Journal report. {open_it}", self.name));
        }
        if report.trim().is_empty() {
            return Err(format!("QB_NO_JOURNAL: {} sent the Journal report back empty. {open_it}", self.name));
        }
        Ok(report.to_string())
    }
}
