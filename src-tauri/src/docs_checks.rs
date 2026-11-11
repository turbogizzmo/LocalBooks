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
    assert_eq!(heading_anchor("\"Tillhouse couldn't open your company file\""), "tillhouse-couldnt-open-your-company-file");
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

    let start = text.find("## How Tillhouse is built to be safe").expect("the built-to-be-safe section");
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

// ------------------------------------------------------------------------------------------------
// Every screen has Help, and every setting is in the guide
// ------------------------------------------------------------------------------------------------

/// The text of a file under `src/components/`.
fn component(name: &str) -> String {
    read(&repo().join("src/components").join(name))
}

/// Every page the app can open: each `{ id: '…' }` in `NAV` (src/components/Sidebar.jsx), the
/// registry the sidebar, the menu bar, Help and saved desktops all open pages from. Groups with
/// children (Sales, Expenses, …) only fold the sidebar and aren't pages.
fn screens() -> Vec<String> {
    let sidebar = component("Sidebar.jsx");
    let start = sidebar.find("export const NAV = [").expect("NAV in Sidebar.jsx");
    let end = start + sidebar[start..].find("\n];").expect("the end of NAV");
    let ids: Vec<String> = regex::Regex::new(r"\{ id: '([\w/-]+)'").unwrap().captures_iter(&sidebar[start..end]).map(|c| c[1].to_string()).collect();
    assert!(ids.len() > 40 && ids.iter().any(|i| i == "dashboard"), "found the pages in NAV ({})", ids.len());
    ids
}

/// Each Help file's `screens` entries: the screen id, then its `help` text and `guide` (empty when
/// missing), and the file it is in.
fn help_screens() -> Vec<(String, String, String, String)> {
    let dir = repo().join("src/components/help");
    let entry = regex::Regex::new(r"(?ms)^  '?([\w/-]+)'?: \{(.*?)^  \},").unwrap();
    let help = regex::Regex::new(r"help:\s*'((?:[^'\\]|\\.)*)'").unwrap();
    let guide = regex::Regex::new(r"guide:\s*'((?:[^'\\]|\\.)*)'").unwrap();
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).expect("read src/components/help") {
        let path = e.expect("read a folder entry").path();
        if path.extension().is_none_or(|x| x != "js") {
            continue;
        }
        let text = read(&path);
        let Some(start) = text.find("export const screens = {") else { continue };
        let end = start + text[start..].find("\n};").expect("the end of screens");
        for c in entry.captures_iter(&text[start..=end]) {
            let body = &c[2];
            let h = help.captures(body).map(|m| m[1].to_string()).unwrap_or_default();
            let g = guide.captures(body).map(|m| m[1].to_string()).unwrap_or_default();
            out.push((c[1].to_string(), h, g, path.file_name().unwrap().to_string_lossy().into_owned()));
        }
    }
    out
}

/// Each page in the sidebar (and so in the menu bar and Help) has its own entry in a Help file's
/// `screens`, with what it is for and a step-by-step guide that exists. F1 on a page shows it.
#[test]
fn every_screen_has_help_and_a_guide() {
    let entries = help_screens();
    let guides = guides();
    let mut problems = Vec::new();
    for id in screens() {
        match entries.iter().find(|(s, ..)| *s == id) {
            None => problems.push(format!("{id}: add it to `screens` in the src/components/help/ file for its area, with `help` (what the screen is for) and `guide`")),
            Some((_, help, guide, file)) => {
                if help.trim().len() < 40 {
                    problems.push(format!("{id} ({file}): `help` should say what the screen is for, in a sentence or two"));
                }
                if guide.is_empty() {
                    problems.push(format!("{id} ({file}): add `guide`, the docs/guide/<name>.md that explains the screen"));
                } else if !guides.contains(guide) {
                    problems.push(format!("{id} ({file}): guide '{guide}' isn't a file in docs/guide/"));
                }
            }
        }
    }
    let ids = screens();
    for (s, _, _, file) in &entries {
        if !ids.contains(s) && s != "settings" {
            problems.push(format!("{s} ({file}): no page has this id in NAV (src/components/Sidebar.jsx); fix or remove it"));
        }
    }
    assert!(problems.is_empty(), "screens without Help:\n{}", problems.join("\n"));
}

/// Help topics open and describe pages that exist: each `views` id and `go` view is a page in NAV
/// (or `settings` / `reports`, which open their first tab).
#[test]
fn help_topics_point_at_screens_that_exist() {
    let ids = screens();
    let views = regex::Regex::new(r"views: \[([^\]]*)\]").unwrap();
    let go = regex::Regex::new(r"view: '([\w/-]+)'").unwrap();
    let quoted = regex::Regex::new(r"'([\w/-]+)'").unwrap();
    let text = help_topics();
    let mut named: Vec<String> = views.captures_iter(&text).flat_map(|c| quoted.captures_iter(&c[1]).map(|q| q[1].to_string()).collect::<Vec<_>>()).collect();
    named.extend(go.captures_iter(&text).map(|c| c[1].to_string()));
    assert!(named.len() > 100, "found the topics' screens ({})", named.len());
    let mut stale: Vec<String> = named.into_iter().filter(|v| !ids.contains(v) && v != "settings").collect();
    stale.sort();
    stale.dedup();
    assert!(stale.is_empty(), "Help topics name screens that don't exist (see NAV in src/components/Sidebar.jsx):\n{}", stale.join("\n"));
}

/// The files that draw Preferences: categories, My Preferences and Company Preferences.
const PREFERENCE_FILES: &[&str] = &["PreferencesWindow.jsx", "TaxTimePreferences.jsx", "GeneralPreferences.jsx", "DesktopViewPreferences.jsx"];

/// Text as a reader sees it: entities decoded, curly apostrophes straight, spaces collapsed, lower case.
fn plain(text: &str) -> String {
    text.replace("&amp;", "&").replace('’', "'").split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Every Preferences setting and heading, as `(kind, label, file)`. Settings are found by the
/// shapes the Preferences files draw them with: `field({ label: '…', prefKey: '…' })` for a company
/// preference, `<Always label="…">`, and `check(key, '…')`, `radio(key, value, '…')` (or `radio(name, checked,
/// () => …, '…')` in Desktop View) and `row(key, '…')` for My Preferences; headings are `<Group title>`, `<Legend>` / `<legend>` and
/// panel titles.
fn preference_settings() -> Vec<(&'static str, String, &'static str)> {
    let shapes: &[(&str, &str)] = &[
        ("setting", r"label: '((?:[^'\\]|\\.)+)',\s*prefKey: '"),
        ("setting", r#"<Always label="([^"]+)""#),
        ("setting", r#"check\('\w+', (?:'((?:[^'\\]|\\.)+)'|"([^"]+)")"#),
        ("setting", r#"radio\(\s*'[\w-]+',\s*[^,]+,\s*\(\) => [^,]+,\s*(?:'((?:[^'\\]|\\.)+)'|"([^"]+)")"#),
        ("setting", r#"radio\('\w+', '\w+', (?:'((?:[^'\\]|\\.)+)'|"([^"]+)")"#),
        ("setting", r"row\('\w+', '([^']+)'"),
        ("heading", r#"<Group title="([^"]+)""#),
        ("heading", r"<Legend>([^<]+)</Legend>"),
        ("heading", r"<legend [^>]*>([^<{]+)</legend>"),
        ("heading", r#"<Panel [^>]*title="([^"]+)""#),
    ];
    let mut out = Vec::new();
    for file in PREFERENCE_FILES {
        let text = component(file);
        for (kind, shape) in shapes {
            for c in regex::Regex::new(shape).unwrap().captures_iter(&text) {
                let label = c.iter().skip(1).flatten().next().expect("a label").as_str();
                out.push((*kind, label.replace("\\'", "'"), *file));
            }
        }
        // A company preference drawn some other way would slip past the check: make it fail here.
        let fields = text.matches("prefKey: '").count();
        let found = regex::Regex::new(shapes[0].1).unwrap().find_iter(&text).count();
        assert_eq!(
            fields, found,
            "{file}: a `prefKey: '…'` isn't written as `label: '…', prefKey: '…'`, so docs_checks can't see its label; write it that way or teach preference_settings() its shape"
        );
    }
    assert!(out.len() > 120, "found the preferences ({})", out.len());
    out
}

/// Each Preferences category has a section in docs/guide/preferences.md, and each setting is named
/// there in bold with its default and what it does. A new setting fails here until it is written up.
#[test]
fn every_preference_is_in_the_preferences_guide() {
    let guide = read(&repo().join("docs/guide/preferences.md"));
    let headings: Vec<String> = guide.lines().filter_map(|l| l.strip_prefix("## ")).map(plain).collect();
    let bold: Vec<String> = guide.split("**").skip(1).step_by(2).map(plain).collect();
    let mut missing = Vec::new();
    let window = component("PreferencesWindow.jsx");
    let categories = &window[window.find("export const CATEGORIES = [").expect("CATEGORIES")..];
    let categories = &categories[..categories.find("\n];").expect("the end of CATEGORIES")];
    let labels: Vec<String> = regex::Regex::new(r"(?m)^    label: '([^']+)'").unwrap().captures_iter(categories).map(|c| c[1].to_string()).collect();
    assert!(labels.len() >= 17, "found the categories ({})", labels.len());
    for label in labels {
        if !headings.contains(&plain(&label)) {
            missing.push(format!("category {label}: add a `## {label}` section"));
        }
    }
    for (kind, label, file) in preference_settings() {
        // Panel titles read "General · My Preferences"; the guide's section and tab say the same.
        let wanted = plain(label.split(" · ").next().unwrap_or(&label));
        let found = match kind {
            "setting" => bold.contains(&wanted),
            // A heading is a `##` section or named in bold, alone or within a bold line such as
            // **Company Preferences: Pricing, Warnings and Sales orders**.
            _ => headings.contains(&wanted) || bold.iter().any(|b| b.contains(&wanted)),
        };
        if !found {
            missing.push(format!("{kind} \"{label}\" ({file}): {}", if kind == "setting" { "name it in bold, with its default and what it does" } else { "mention it under its category" }));
        }
    }
    assert!(missing.is_empty(), "docs/guide/preferences.md is missing:\n{}", missing.join("\n"));
}

/// The components a Settings page draws, following `./X.jsx` imports from Settings.jsx (Preferences
/// has its own check above).
fn settings_components() -> Vec<String> {
    let import = regex::Regex::new(r"from '\./(\w+\.jsx)'").unwrap();
    let mut seen: Vec<String> = Vec::new();
    let mut todo = vec!["Settings.jsx".to_string()];
    while let Some(file) = todo.pop() {
        if seen.contains(&file) || PREFERENCE_FILES.contains(&file.as_str()) {
            continue;
        }
        todo.extend(import.captures_iter(&component(&file)).map(|c| c[1].to_string()));
        seen.push(file);
    }
    assert!(seen.len() > 10, "found the Settings components ({})", seen.len());
    seen
}

/// Each section of a Settings page (a `<Group title>`, a `<Section title>` or a static `<h2>`) is in
/// the guide, as a `##` / `###` heading or in bold (alone or as a step of a menu path), so a new
/// section can't ship unexplained.
#[test]
fn every_settings_section_is_in_the_guide() {
    let mut marks = Vec::new();
    for g in guides() {
        let text = read(&repo().join(format!("docs/guide/{g}.md")));
        marks.extend(text.lines().filter_map(|l| l.strip_prefix("## ").or_else(|| l.strip_prefix("### "))).map(plain));
        // A bold menu path names each step: **Settings › Updates** names Updates.
        for bold in text.split("**").skip(1).step_by(2) {
            marks.push(plain(bold));
            marks.extend(bold.split(" › ").map(plain));
        }
    }
    let shapes = [r#"<(?:Group|Section) title="([^"]+)""#, r"<h2[^>]*>([^<{]+)</h2>"];
    let mut missing = Vec::new();
    for file in settings_components() {
        let text = component(&file);
        for shape in shapes {
            for c in regex::Regex::new(shape).unwrap().captures_iter(&text) {
                // "File Check & Repair · Verify & Rebuild": the part before the dot is its name.
                let title = plain(c[1].split(" · ").next().unwrap_or_default());
                if !title.is_empty() && !marks.contains(&title) {
                    missing.push(format!("\"{}\" ({file}): explain it in docs/guide/, under a heading or in bold", c[1].trim()));
                }
            }
        }
    }
    assert!(missing.is_empty(), "Settings sections not in the guide:\n{}", missing.join("\n"));
}

/// Every command and submenu LocalBooks adds to the menu bar: the labels in `app_menu.rs`
/// (`page`, `form`, `keyed`, `report`, `form_cmd`, `Entry::Cmd`, `Entry::Go`, `Entry::Sub`),
/// `file_menu.rs` and the Window and View items in `windows.rs`.
fn menu_commands() -> Vec<(String, &'static str)> {
    let shapes = [
        r#"\b(?:page|form|keyed|report|form_cmd)\("([^"]+)""#,
        r#"Entry::(?:Cmd|Go) \{ label: "([^"]+)""#,
        r#"Entry::Sub\(\s*"([^"]+)""#,
        r#"(?:MenuItem|CheckMenuItem)::with_id\(app, \w+, "([^"]+)""#,
        r#"Submenu::(?:new|with_items)\(app, "([^"]+)""#,
        r#"\("[\w-]+", "([A-Z][^"]+)"\)"#,
    ];
    let mut out = Vec::new();
    for file in ["app_menu.rs", "file_menu.rs", "windows.rs"] {
        let text = read(&repo().join("src-tauri/src").join(file));
        // The menus, not the tests that follow them.
        let code = text.split("#[cfg(test)]").next().unwrap_or_default();
        for shape in shapes {
            for c in regex::Regex::new(shape).unwrap().captures_iter(code) {
                let label = c[1].to_string();
                if !out.iter().any(|(l, _)| *l == label) {
                    out.push((label, file));
                }
            }
        }
    }
    // File is the platform's own menu (file_menu.rs finds it by name); its name isn't a command.
    out.retain(|(l, _)| l != "File");
    assert!(out.len() > 200, "found the menu commands ({})", out.len());
    out
}

/// docs/guide/menus.md names every menu command in bold (alone, or as a step of a menu path such as
/// **Utilities › Export › Lists to IIF Files…**) and says where it goes.
#[test]
fn every_menu_command_is_in_the_menu_guide() {
    let guide = read(&repo().join("docs/guide/menus.md"));
    let mut named: Vec<String> = guide.lines().filter_map(|l| l.strip_prefix("## ")).map(plain).collect();
    for bold in guide.split("**").skip(1).step_by(2) {
        named.push(plain(bold));
        named.extend(bold.split(" › ").map(plain));
    }
    let missing: Vec<String> = menu_commands()
        .into_iter()
        .filter(|(label, _)| !named.contains(&plain(label)))
        .map(|(label, file)| format!("\"{label}\" ({file}): add it to docs/guide/menus.md in bold, with what it opens"))
        .collect();
    assert!(missing.is_empty(), "menu commands not in the menu guide:\n{}", missing.join("\n"));
}

/// Every `guide:` in a Help file is a bare guide name that exists, so a slip that pastes text into
/// the field (`guide: 'attachments The Attachments Library…'`) fails here instead of hiding the
/// topic's guide link and its sentence.
#[test]
fn every_help_guide_field_names_a_guide() {
    let guides = guides();
    let field = regex::Regex::new(r"\bguide:\s*'((?:[^'\\]|\\.)*)'").unwrap();
    let mut problems = Vec::new();
    for e in std::fs::read_dir(repo().join("src/components/help")).expect("read src/components/help") {
        let path = e.expect("read a folder entry").path();
        if path.extension().is_none_or(|x| x != "js") {
            continue;
        }
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        for c in field.captures_iter(&read(&path)) {
            let name = &c[1];
            if !guides.iter().any(|g| g == name) {
                let shown: String = name.chars().take(60).collect();
                problems.push(format!("{file}: guide: '{shown}' isn't the name of a docs/guide/<name>.md"));
            }
        }
    }
    assert!(problems.is_empty(), "Help guide links that don't resolve:\n{}", problems.join("\n"));
}
