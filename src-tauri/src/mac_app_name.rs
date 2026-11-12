//! Gives an updated Mac copy its new name. Until 0.16 the app was LocalBooks, and the updater
//! replaces a bundle where it is, so a copy updated from LocalBooks stays `LocalBooks.app` in the
//! Applications folder while showing Tillhouse everywhere else (`docs/plans/rebrand.md`).
//!
//! On launch, before any window, a bundle named `LocalBooks.app` renames itself to `Tillhouse.app`
//! beside it and starts again from there (`exec`, same process, same arguments), so nothing in
//! the running app keeps the old path: the updater and Restart work from the bundle's real place.
//! Company files, settings and saved passwords don't depend on the bundle's name or place.
//! Nothing happens if `Tillhouse.app` is already there (say, a fresh download beside an old copy),
//! or if the folder can't be written; the app then runs from `LocalBooks.app` as before.

use std::path::{Path, PathBuf};

#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "used on macOS and in tests only"))]
const OLD: &str = "LocalBooks.app";
#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "used on macOS and in tests only"))]
const NEW: &str = "Tillhouse.app";

/// For an executable inside `…/LocalBooks.app/Contents/MacOS/`, the bundle and the renamed
/// bundle beside it, and the executable's path inside the renamed bundle. Otherwise `None`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "used on macOS and in tests only"))]
pub fn plan(exe: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    if macos.file_name()? != "MacOS" || contents.file_name()? != "Contents" || bundle.file_name()? != OLD {
        return None;
    }
    let renamed = bundle.with_file_name(NEW);
    let new_exe = renamed.join(exe.strip_prefix(bundle).ok()?);
    Some((bundle.to_path_buf(), renamed, new_exe))
}

/// Renames the running bundle and restarts from the new name, or returns to carry on as is.
#[cfg(target_os = "macos")]
pub fn adopt_new_name() {
    use std::os::unix::process::CommandExt;

    let Ok(exe) = std::env::current_exe() else { return };
    let Some((bundle, renamed, new_exe)) = plan(&exe) else { return };
    if renamed.exists() {
        return;
    }
    if let Err(err) = std::fs::rename(&bundle, &renamed) {
        eprintln!("Kept the app's old name {}: {err}", bundle.display());
        return;
    }
    // Only returns if the restart failed; the bundle is already renamed, so carry on in this process.
    let err = std::process::Command::new(&new_exe).args(std::env::args_os().skip(1)).exec();
    eprintln!("Renamed the app to {} but could not restart from it: {err}", renamed.display());
}

#[cfg(not(target_os = "macos"))]
pub fn adopt_new_name() {}

#[cfg(test)]
mod tests {
    use super::plan;
    use std::path::Path;

    #[test]
    fn renames_only_a_localbooks_bundle() {
        let (bundle, renamed, exe) = plan(Path::new("/Applications/LocalBooks.app/Contents/MacOS/localbooks")).unwrap();
        assert_eq!(bundle, Path::new("/Applications/LocalBooks.app"));
        assert_eq!(renamed, Path::new("/Applications/Tillhouse.app"));
        assert_eq!(exe, Path::new("/Applications/Tillhouse.app/Contents/MacOS/localbooks"));
        // Anywhere the user keeps it, not just /Applications.
        assert!(plan(Path::new("/Users/pat/Apps/LocalBooks.app/Contents/MacOS/localbooks")).is_some());

        for other in [
            "/Applications/Tillhouse.app/Contents/MacOS/localbooks",
            "/Applications/LocalBooks Copy.app/Contents/MacOS/localbooks",
            "/Users/pat/LocalBooks.app/localbooks",
            "/usr/bin/localbooks",
            "localbooks",
            "C:\\Program Files\\LocalBooks\\localbooks.exe",
        ] {
            assert!(plan(Path::new(other)).is_none(), "{other}");
        }
    }
}
