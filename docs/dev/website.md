# The website (tillhouse.app)

The public site is one static page, `site/index.html`, with its icon (`site/icon.png`), favicon
(`site/favicon.png`), screenshots (`site/shots/`) and `site/CNAME`. There is no build step, framework or web font: system fonts,
inline CSS and one small script. Open the file in a browser to preview it.

## What is on the page

- The hero and the download button. The script asks the GitHub API for the newest published
  release (previews included) and links the file for the visitor's system: `-setup.exe`, `.dmg`,
  `.AppImage`, plus the `.msi`, `.deb` and `.rpm`. File names carry the version, so there are no
  fixed direct links; if the API can't be reached, the button opens the releases page.
- Features, Moving from QuickBooks Desktop (the program chooser, with a table as the no-script
  fallback), Install, What's new to test, Updates, Report a problem, Privacy, and the guide list.
- Screenshots in `site/shots/` (1600 × 1000 JPEGs): `home.jpg` is the hero, `invoices.jpg` and
  `report.jpg` sit above the features. They show Tillhouse on the seeded **Five Years Test Co**
  (`src-tauri/src/test_companies.rs`), synthetic test data. Any new screenshot must be of Tillhouse
  on a synthetic test company, never of a real company file or of another product. Give each one
  alt text, `width` and `height`, and `loading="lazy"` unless it is the hero.
- The trademark notice in the footer. Keep it, as in README.md and About (AGENTS.md rule 11).

`docs_checks` fails a PR if a guide in `docs/guide/` is missing from the guide list, or if the
**What's new to test in <major.minor>** heading doesn't match `package.json`'s version.

Colors are the app's Spruce & Brass tokens (`tailwind.config.js`), with a dark palette under
`prefers-color-scheme: dark`. Check new text against WCAG AA (4.5:1) in both.

## How it deploys

`.github/workflows/pages.yml` runs on every push to `main` that changes `site/**` (or by hand). It
uploads the `site/` folder as the Pages artifact and deploys it with `actions/deploy-pages`. Pages
serves whatever is in `site/`; nothing else in the repo is published.

Because the site deploys from a workflow, GitHub reads the custom domain from the repository's Pages
settings, not from `site/CNAME`. The file is kept so the folder says where it is served, and so
the site still works if it is ever published from a branch.

## Go-live checklist for tillhouse.app

Steps 1, 2, 4 and 5 are done by the repository owner. `.app` domains only work over HTTPS (the
whole TLD is on browsers' HSTS preload list), so the site is unreachable until step 5's certificate
exists.

1. **DNS records at the registrar** for `tillhouse.app`:
   - `A` records for the apex `@`: `185.199.108.153`, `185.199.109.153`, `185.199.110.153`,
     `185.199.111.153`.
   - `AAAA` records for `@`: `2606:50c0:8000::153`, `2606:50c0:8001::153`, `2606:50c0:8002::153`,
     `2606:50c0:8003::153`.
   - `CNAME` for `www` → `turbogizzmo.github.io`.
   - `tillhouse.net` forwards to `https://tillhouse.app` at the registrar; it needs nothing here.
2. **Verify the domain in GitHub** (account Settings › Pages › Add a domain): add the `TXT`
   record GitHub shows (`_github-pages-challenge-turbogizzmo.tillhouse.app`) and click Verify.
   This stops anyone else's Pages site from claiming the domain.
3. **The CNAME file**: `site/CNAME` contains `tillhouse.app`. Done in the rebrand PR.
4. **Set the custom domain on the repository's Pages site:**

   ```bash
   gh api -X PUT repos/turbogizzmo/LocalBooks/pages -f cname=tillhouse.app
   ```

5. **Wait for the certificate, then enforce HTTPS.** GitHub requests a Let's Encrypt certificate
   once DNS resolves (minutes to an hour). Check with
   `gh api repos/turbogizzmo/LocalBooks/pages --jq '.https_certificate.state'` until it says
   `approved`, then:

   ```bash
   gh api -X PUT repos/turbogizzmo/LocalBooks/pages -F https_enforced=true
   ```

   Then open `https://tillhouse.app` and `https://www.tillhouse.app` and check the page and the
   download button.

After go-live, the old address `turbogizzmo.github.io/LocalBooks/` redirects to the custom domain.
