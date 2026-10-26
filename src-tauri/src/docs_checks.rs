//! Checks that the user-facing documentation keeps up with the code: every guide is reachable from
//! the tester page, the in-app Help and the guide index, and a version bump comes with its
//! changelog section and the tester page's "What's new". A feature or release PR that forgets one
//! fails here instead of shipping stale docs.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The guide names (`docs/guide/<name>.md`), without the index.
fn guides() -> Vec<String> {
    let dir = repo().join("docs/guide");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("read docs/guide")
        .map(|e| e.expect("read a folder entry").file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md") && n != "README.md")
        .map(|n| n.trim_end_matches(".md").to_string())
        .collect();
    names.sort();
    assert!(names.len() > 10, "found the guides in {}", dir.display());
    names
}

/// Every Help topic file under `src/components/help/`, concatenated.
fn help_topics() -> String {
    let dir = repo().join("src/components/help");
    let mut text = String::new();
    for entry in std::fs::read_dir(&dir).expect("read src/components/help") {
        let path = entry.expect("read a folder entry").path();
        if path.extension().is_some_and(|e| e == "js") {
            text.push_str(&read(&path));
        }
    }
    assert!(!text.is_empty(), "found the help topics in {}", dir.display());
    text
}

#[test]
fn every_guide_is_linked_from_the_site_help_and_index() {
    let site = read(&repo().join("site/index.html"));
    let help = help_topics();
    let index = read(&repo().join("docs/guide/README.md"));
    let mut missing = Vec::new();
    for g in guides() {
        if !site.contains(&format!("docs/guide/{g}.md")) {
            missing.push(format!("{g}: add it to the guide list in site/index.html"));
        }
        if !help.contains(&format!("guide: '{g}'")) {
            missing.push(format!("{g}: add a Help topic with guide: '{g}' under src/components/help/"));
        }
        if !index.contains(&format!("{g}.md")) {
            missing.push(format!("{g}: link it from docs/guide/README.md"));
        }
    }
    assert!(missing.is_empty(), "guides not reachable:\n{}", missing.join("\n"));
}

#[test]
fn the_version_has_its_changelog_and_whats_new() {
    let package = read(&repo().join("package.json"));
    let version = package
        .lines()
        .find_map(|l| l.trim().strip_prefix("\"version\": \"").and_then(|r| r.split('"').next()))
        .expect("package.json has a version")
        .to_string();
    let minor = version.splitn(3, '.').take(2).collect::<Vec<_>>().join(".");
    let changelog = read(&repo().join("CHANGELOG.md"));
    assert!(
        changelog.contains(&format!("## [{version}]")),
        "CHANGELOG.md needs a `## [{version}]` section: it becomes the release notes"
    );
    let site = read(&repo().join("site/index.html"));
    assert!(
        site.contains(&format!("What’s new to test in {minor}<")),
        "site/index.html's What's new should be for {minor}: update it in the release PR"
    );
}
