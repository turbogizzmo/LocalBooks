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

/// The relative link targets (`](path)`) in a Markdown text, without anchors.
fn relative_links(text: &str) -> Vec<String> {
    text.split("](")
        .skip(1)
        .filter_map(|rest| rest.split(')').next())
        .filter(|target| !target.starts_with("http") && !target.starts_with('#'))
        .map(|target| target.split('#').next().unwrap_or_default().to_string())
        .collect()
}

/// R6: every claim in `SECURITY.md`'s "how it is built" and "users" sections links to the code or
/// test that backs it, every link resolves, and every function, test or constant it names in
/// backticks appears in one of the files it links.
#[test]
fn security_claims_point_at_code_that_exists() {
    let text = read(&repo().join("SECURITY.md"));
    let mut problems = Vec::new();
    for link in relative_links(&text) {
        if !repo().join(&link).exists() {
            problems.push(format!("link to {link}, which does not exist"));
        }
    }
    let code: String = relative_links(&text).iter().filter(|l| l.ends_with(".rs") || l.ends_with(".jsx") || l.ends_with(".json")).filter_map(|l| std::fs::read_to_string(repo().join(l)).ok()).collect();

    let start = text.find("## How LocalBooks is built to be safe").expect("the built-to-be-safe section");
    let end = text.find("What this does not cover:").expect("the limits list");
    let claims = &text[start..end];
    let mut bullets: Vec<String> = Vec::new();
    for line in claims.lines() {
        if line.starts_with("- ") {
            bullets.push(line.to_string());
        } else if line.starts_with("  ") {
            if let Some(last) = bullets.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        }
    }
    assert!(bullets.len() >= 15, "found the claims ({})", bullets.len());
    for bullet in &bullets {
        if !bullet.contains("](src") {
            problems.push(format!("claim without a link to code: {}", &bullet[..bullet.len().min(80)]));
        }
        // Names in backticks that look like code (snake_case or Type::name) must exist.
        for (i, name) in bullet.split('`').enumerate() {
            let looks_like_code = i % 2 == 1 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':') && (name.contains('_') || name.contains("::"));
            let last = name.rsplit("::").next().unwrap_or(name);
            if looks_like_code && !code.contains(last) {
                problems.push(format!("`{name}` is not in any file SECURITY.md links"));
            }
        }
    }
    assert!(problems.is_empty(), "SECURITY.md:\n{}", problems.join("\n"));
}
