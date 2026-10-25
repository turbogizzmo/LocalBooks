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
