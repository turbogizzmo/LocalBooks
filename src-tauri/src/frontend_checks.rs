//! Checks on the screens' source (`src/`) that no screen test can make on every system.

use std::path::{Path, PathBuf};

/// The `.js` and `.jsx` files under `dir`. Folders with a space in their name are skipped: they are
/// Finder or iCloud duplicates ("components 2"), not part of the app.
fn script_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read the screens' source folder") {
        let path = entry.expect("read a folder entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if path.is_dir() {
            if !name.contains(' ') {
                script_files(&path, out);
            }
        } else if name.ends_with(".js") || name.ends_with(".jsx") {
            out.push(path);
        }
    }
}

/// The macOS webview shows nothing for `window.confirm`, `window.prompt` or `window.alert`:
/// confirm() returns false and prompt() null at once, so the action silently never happens (New
/// Recovery Key, deleting a budget, starting an import over). Screens ask with `lib/dialogs.jsx`.
#[test]
fn screens_never_use_the_browsers_own_dialogs() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let mut files = Vec::new();
    script_files(&src, &mut files);
    assert!(files.len() > 20, "found the screens' source in {}", src.display());
    let mut found = Vec::new();
    for file in files.iter().filter(|f| !f.ends_with("lib/dialogs.jsx")) {
        let text = std::fs::read_to_string(file).expect("read a source file");
        for (n, line) in text.lines().enumerate() {
            if ["window.confirm(", "window.prompt(", "window.alert("].iter().any(|call| line.contains(call)) {
                found.push(format!("{}:{}", file.strip_prefix(&src).unwrap().display(), n + 1));
            }
        }
    }
    assert!(found.is_empty(), "use askConfirm/askText from lib/dialogs.jsx instead of: {}", found.join(", "));
}

/// `lib/api.js` merges the files under `lib/api/` into one object, so a name defined in two files
/// would silently keep only one of them.
#[test]
fn backend_wrappers_are_named_once() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/api");
    let mut files: Vec<_> = std::fs::read_dir(&dir).expect("read lib/api").map(|e| e.expect("read a folder entry").path()).collect();
    files.sort();
    assert!(files.len() > 20, "found the wrappers in {}", dir.display());
    let mut seen = std::collections::BTreeMap::new();
    let mut twice = Vec::new();
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        for line in std::fs::read_to_string(file).expect("read a wrapper file").lines() {
            let Some(rest) = line.strip_prefix("  ") else { continue };
            let Some((key, _)) = rest.split_once(':') else { continue };
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            if let Some(first) = seen.insert(key.to_string(), name.clone()) {
                twice.push(format!("{key} ({first}, {name})"));
            }
        }
    }
    assert!(seen.len() > 200, "found the wrappers");
    assert!(twice.is_empty(), "defined in two files: {}", twice.join(", "));
}

/// Multiple Windows: a list that opens a transaction in a window of its own sends an `edit` request
/// with the transaction's id (`openFromList('expenses/bills', 'edit', { billId: … })`). That window,
/// and the saved desktop that reopens it, show the screen in the same file, which must read the same
/// id back (`useEditIntent(intent, 'billId', …)` or `intent.data?.billId`), or the window would open
/// on an empty form.
#[test]
fn every_transaction_opened_in_its_own_window_is_read_back() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
    let mut files = Vec::new();
    script_files(&src, &mut files);
    let mut sent = 0;
    let mut unread = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("read a source file");
        for call in text.split("openFromList(").skip(1) {
            let Some(rest) = call.split_once("'edit', { ").map(|(_, r)| r) else { continue };
            let Some((key, _)) = rest.split_once(':') else { continue };
            sent += 1;
            if !text.contains(&format!("useEditIntent(intent, '{key}'")) && !text.contains(&format!("intent.data?.{key}")) {
                unread.push(format!("{} ({key})", file.strip_prefix(&src).unwrap().display()));
            }
        }
    }
    assert!(sent >= 3, "found the lists that open transactions in their own window");
    assert!(unread.is_empty(), "these screens open a transaction in its own window but never read its id back: {}", unread.join(", "));
}
