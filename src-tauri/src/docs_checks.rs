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

/// A heading's anchor as GitHub makes it (and as `headingId` in GuideView.jsx does in the app).
fn heading_anchor(heading: &str) -> String {
    heading
        .replace("**", "")
        .replace('`', "")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-')
        .collect::<String>()
        .trim()
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect()
}

#[test]
fn guide_links_point_at_guides_and_headings_that_exist() {
    let link = regex::Regex::new(r"\]\(([\w-]+)\.md(?:#([\w-]+))?\)").unwrap();
    let mut names = guides();
    names.push("README".into());
    let anchors = |name: &str| -> Vec<String> {
        read(&repo().join(format!("docs/guide/{name}.md")))
            .lines()
            .filter_map(|l| l.strip_prefix("## ").or_else(|| l.strip_prefix("### ")))
            .map(heading_anchor)
            .collect()
    };
    let mut broken = Vec::new();
    for from in &names {
        let text = read(&repo().join(format!("docs/guide/{from}.md")));
        for c in link.captures_iter(&text) {
            let to = &c[1];
            if !names.iter().any(|n| n == to) {
                broken.push(format!("{from}.md links to {to}.md, which doesn't exist"));
            } else if let Some(section) = c.get(2) {
                if !anchors(to).iter().any(|a| a == section.as_str()) {
                    broken.push(format!("{from}.md links to {to}.md#{}, which has no such heading", section.as_str()));
                }
            }
        }
    }
    assert!(broken.is_empty(), "broken guide links:\n{}", broken.join("\n"));
}

#[test]
fn heading_anchors_match_github() {
    assert_eq!(heading_anchor("Accountant & Taxes reports"), "accountant--taxes-reports");
    assert_eq!(heading_anchor("\"LocalBooks couldn't open your company file\""), "localbooks-couldnt-open-your-company-file");
    assert_eq!(heading_anchor("See the transactions behind a number (QuickZoom)"), "see-the-transactions-behind-a-number-quickzoom");
}
