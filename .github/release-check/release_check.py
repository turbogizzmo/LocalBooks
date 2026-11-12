#!/usr/bin/env python3
"""Release check driver for .github/workflows/release-check.yml (see docs/testing/release-check.md).

Subcommands, one per workflow step. Each appends its part of the job summary to
$GITHUB_STEP_SUMMARY (or prints it when run by hand) and exits 1 when a check fails.

  artifacts  verify latest.json and every downloaded installer against the updater public key, and
             that latest.json's download URLs resolve to the same signed bytes
  install    install this platform's release installer and print the app's executable path
  codesign   report (never fail on) `codesign --verify` of the installed macOS app
  balances   record the balances of freshly seeded company files
  upgrade    run the new release's `--check-file` on a copy of each seeded company
  launch     start the installed app on a copy of each seeded company, check it is still running
             after LAUNCH_SECONDS with no crash, quit it, then verify the file it upgraded
  installer-upgrade
             install the previous release, apply this one over it as the updater does, and check
             one copy is left, it opens each seeded company, and the app data folder is kept
"""

import argparse
import base64
import hashlib
import json
import os
import platform
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

LAUNCH_SECONDS = 30
CHECK_TIMEOUT = 600
INSTALL_TIMEOUT = 600
APP_ID = "com.localbooks.desktop"
# The installers and update bundles each platform's job checks, by file-name suffix.
INSTALLERS = {
    "darwin": [".dmg", ".app.tar.gz"],
    "windows": ["-setup.exe", ".msi"],
    "linux": [".deb", ".rpm", ".AppImage"],
}
# latest.json platform keys that must be present for each platform.
REQUIRED_UPDATER_KEYS = {
    "darwin": ["darwin-aarch64", "darwin-x86_64"],
    "windows": ["windows-x86_64"],
    "linux": ["linux-x86_64"],
}

failures = []


def summary(text=""):
    """Adds to the job summary, and to the log so the results are also in the run's logs."""
    print(text, flush=True)
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a", encoding="utf-8") as f:
            f.write(text + "\n")


def fail(message):
    failures.append(message)
    print(f"::error::{message}", flush=True)


def finish():
    if failures:
        summary(f"\n**{len(failures)} problem(s):**\n")
        for message in failures:
            summary(f"- {message}")
    sys.exit(1 if failures else 0)


def mark(ok):
    return "✅" if ok else "❌"


# ------------------------------------------------------------------------------------------------
# Minisign (what Tauri's updater uses): verify a signature against the public key.
# ------------------------------------------------------------------------------------------------


def minisign_public_key(config_pubkey):
    """(key id, Ed25519 public key) from tauri.conf.json's base64 `pubkey`."""
    lines = base64.b64decode(config_pubkey).decode().splitlines()
    blob = base64.b64decode(lines[1])
    if blob[:2] != b"Ed" or len(blob) != 42:
        raise ValueError("not a minisign Ed25519 public key")
    return blob[2:10], blob[10:]


def minisign_verify(public_key, signature_b64, data):
    """Checks a Tauri signature (base64 of a minisign .sig file) over `data`. Returns the trusted
    comment, or raises ValueError."""
    from cryptography.exceptions import InvalidSignature
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

    key_id, key = public_key
    lines = base64.b64decode(signature_b64).decode().splitlines()
    blob = base64.b64decode(lines[1])
    algorithm, sig_key_id, sig = blob[:2], blob[2:10], blob[10:74]
    if sig_key_id != key_id:
        raise ValueError(f"signed with key {sig_key_id[::-1].hex().upper()}, expected {key_id[::-1].hex().upper()}")
    if algorithm == b"ED":
        message = hashlib.blake2b(data, digest_size=64).digest()
    elif algorithm == b"Ed":
        message = data
    else:
        raise ValueError(f"unknown signature algorithm {algorithm!r}")
    prefix = "trusted comment: "
    if not lines[2].startswith(prefix):
        raise ValueError("no trusted comment")
    trusted = lines[2][len(prefix):]
    verifier = Ed25519PublicKey.from_public_bytes(key)
    try:
        verifier.verify(sig, message)
    except InvalidSignature:
        raise ValueError("signature does not match the file") from None
    try:
        verifier.verify(base64.b64decode(lines[3]), sig + trusted.encode())
    except InvalidSignature:
        raise ValueError("trusted comment signature does not match") from None
    return trusted


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def download(url, token):
    """GETs an updater URL as the updater does (Accept: application/octet-stream). The token goes
    to GitHub's API only, never to the storage host it redirects to."""
    headers = {"Accept": "application/octet-stream", "User-Agent": "localbooks-release-check"}
    if token and url.startswith("https://api.github.com/"):
        headers["Authorization"] = f"Bearer {token}"
    opener = urllib.request.build_opener(NoRedirect)
    for _ in range(5):
        try:
            with opener.open(urllib.request.Request(url, headers=headers), timeout=120) as response:
                return response.read()
        except urllib.error.HTTPError as e:
            if e.code in (301, 302, 303, 307, 308) and e.headers.get("Location"):
                url = e.headers["Location"]
                headers = {"Accept": "application/octet-stream", "User-Agent": "localbooks-release-check"}
                continue
            raise
    raise ValueError("too many redirects")


def cmd_artifacts(args):
    release = Path(args.release_dir)
    pubkey = json.loads(Path(args.config).read_text())["plugins"]["updater"]["pubkey"]
    summary(f"### Artifacts ({args.platform})\n")
    if args.previous_config:
        previous = json.loads(Path(args.previous_config).read_text())["plugins"]["updater"]["pubkey"]
        if previous != pubkey:
            fail("the updater public key changed since the previous release: installed copies would refuse this update")
        else:
            summary("- Updater public key is the same as the previous release's ✅")
    key = minisign_public_key(pubkey)
    summary(f"- Updater key id `{key[0][::-1].hex().upper()}`\n")

    # Every installer and update bundle attached for this platform, against its .sig file.
    summary("| File | Size | Signature |\n|---|---:|---|")
    suffixes = INSTALLERS[args.platform]
    files = sorted(p for p in release.iterdir() if any(p.name.endswith(s) for s in suffixes))
    if not files:
        fail(f"no {args.platform} installers in the release")
    by_name = {}
    for path in files:
        data = path.read_bytes()
        by_name[path.name] = data
        sig = path.with_name(path.name + ".sig")
        if not sig.exists():
            note = "no .sig (not an update bundle)" if path.suffix == ".dmg" else "❌ missing .sig"
            if path.suffix != ".dmg":
                fail(f"{path.name} has no .sig file")
        else:
            try:
                trusted = minisign_verify(key, sig.read_text().strip(), data)
                note = f"✅ `{trusted.split(chr(9))[1] if chr(9) in trusted else trusted}`"
            except ValueError as e:
                note = f"❌ {e}"
                fail(f"{path.name}: {e}")
        summary(f"| `{path.name}` | {len(data) / 1e6:.1f} MB | {note} |")

    # latest.json: version, required platforms, and each URL downloads the signed bytes.
    latest = json.loads((release / "latest.json").read_text())
    summary(f"\n`latest.json` version **{latest.get('version')}**\n")
    if latest.get("version") != args.version:
        fail(f"latest.json says version {latest.get('version')}, the release is {args.version}")
    platforms = latest.get("platforms", {})
    for required in REQUIRED_UPDATER_KEYS[args.platform]:
        if required not in platforms:
            fail(f"latest.json has no `{required}` entry")
    summary("| Updater platform | URL resolves | Same as release asset | Signature |\n|---|---|---|---|")
    fetched = {}
    for name, entry in sorted(platforms.items()):
        if not name.startswith(args.platform):
            continue
        url = entry["url"]
        resolves, same, sig_note = False, "—", ""
        try:
            if url not in fetched:
                fetched[url] = download(url, os.environ.get("GH_TOKEN"))
            data = fetched[url]
            resolves = True
            matches = [n for n, d in by_name.items() if d == data]
            same = f"✅ `{matches[0]}`" if matches else "❌ no match"
            if not matches:
                fail(f"latest.json `{name}` downloads a file that is not one of the release's assets")
            trusted = minisign_verify(key, entry["signature"], data)
            sig_note = "✅ " + trusted.split("\t")[1] if "\t" in trusted else "✅"
        except (urllib.error.URLError, ValueError, OSError) as e:
            sig_note = f"❌ {e}"
            fail(f"latest.json `{name}` ({url}): {e}")
        summary(f"| `{name}` | {mark(resolves)} | {same} | {sig_note} |")
    summary()
    finish()


# ------------------------------------------------------------------------------------------------
# Installing the release
# ------------------------------------------------------------------------------------------------


def only(release, suffix):
    matches = sorted(p for p in Path(release).iterdir() if p.name.endswith(suffix))
    if len(matches) != 1:
        raise SystemExit(f"expected one *{suffix} in {release}, found {[m.name for m in matches]}")
    return matches[0]


def cmd_install(args):
    if args.platform == "darwin":
        dmg = only(args.release_dir, ".dmg")
        mount = Path(tempfile.mkdtemp(prefix="lb-dmg-"))
        subprocess.run(["hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", str(mount), str(dmg)], check=True, stdout=subprocess.DEVNULL)
        try:
            app = next(mount.glob("*.app"))
            target = Path(args.dest) / app.name
            if target.exists():
                shutil.rmtree(target)
            Path(args.dest).mkdir(parents=True, exist_ok=True)
            subprocess.run(["ditto", str(app), str(target)], check=True)
        finally:
            subprocess.run(["hdiutil", "detach", str(mount)], check=False, stdout=subprocess.DEVNULL)
        executable = subprocess.run(["plutil", "-extract", "CFBundleExecutable", "raw", str(target / "Contents/Info.plist")], check=True, capture_output=True, text=True).stdout.strip()
        exe = target / "Contents/MacOS" / executable
    elif args.platform == "windows":
        setup = only(args.release_dir, "-setup.exe")
        subprocess.run([str(setup), "/S"], check=True)
        # The installer names the folder after the product: Tillhouse, or LocalBooks before 0.16.
        local, program_files = Path(os.environ["LOCALAPPDATA"]), Path(os.environ.get("ProgramFiles", "C:/Program Files"))
        roots = [base / name for name in ("Tillhouse", "LocalBooks") for base in (local, program_files)]
        candidates = [p for root in roots if root.is_dir() for p in root.glob("*.exe") if not p.name.lower().startswith("uninstall")]
        if not candidates:
            raise SystemExit(f"the installer finished but no Tillhouse executable was found in {roots}")
        exe = candidates[0]
    else:
        deb = only(args.release_dir, ".deb")
        subprocess.run(["sudo", "apt-get", "install", "-y", "--no-install-recommends", str(deb.resolve())], check=True, stdout=subprocess.DEVNULL)
        package = subprocess.run(["dpkg-deb", "-f", str(deb), "Package"], check=True, capture_output=True, text=True).stdout.strip()
        files = subprocess.run(["dpkg", "-L", package], check=True, capture_output=True, text=True).stdout.splitlines()
        binaries = [f for f in files if f.startswith("/usr/bin/") and Path(f).is_file()]
        if not binaries:
            raise SystemExit(f"the {package} package installs nothing in /usr/bin: {files}")
        exe = Path(binaries[0])
    print(exe)


def cmd_codesign(args):
    app = Path(args.app)
    while app.suffix != ".app":
        app = app.parent
    result = subprocess.run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)], capture_output=True, text=True)
    details = subprocess.run(["codesign", "-dv", str(app)], capture_output=True, text=True).stderr
    output = (result.stdout + result.stderr).strip()
    summary("### Code signature (macOS)\n")
    if result.returncode == 0:
        summary(f"- `codesign --verify` passed ✅\n\n```\n{details.strip()}\n```\n")
    else:
        # Expected until a Developer ID certificate is set up: reported, never a failure.
        print(f"::notice::codesign --verify failed (expected while builds are unsigned): {output}")
        summary(f"- `codesign --verify` did not pass: expected while macOS builds are not code-signed ⚠️ (not a failure)\n\n```\n{output}\n{details.strip()}\n```\n")


# ------------------------------------------------------------------------------------------------
# Company files
# ------------------------------------------------------------------------------------------------


def companies(seed_dir):
    return sorted(Path(seed_dir).glob("*.localbooks"))


def copy_company(source, target_dir):
    target_dir.mkdir(parents=True, exist_ok=True)
    target = target_dir / source.name
    for suffix in ["", "-wal", "-shm"]:
        src = Path(f"{source}{suffix}")
        if src.exists():
            shutil.copy2(src, f"{target}{suffix}")
    return target


def read_company(path):
    """Balances and health of a company file, read from a scratch copy (so the file itself and its
    journal are never touched)."""
    scratch = copy_company(Path(path), Path(tempfile.mkdtemp(prefix="lb-read-")))
    con = sqlite3.connect(scratch)
    try:
        balances = {
            f"{account_id} {name} [{status}]": cents
            for account_id, name, status, cents in con.execute(
                """SELECT a.id, a.name, e.status, SUM(l.debit_cents) - SUM(l.credit_cents)
                   FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
                   JOIN chart_of_accounts a ON a.id = l.account_id
                   GROUP BY a.id, e.status ORDER BY a.id, e.status"""
            )
        }
        return {
            "schema": con.execute("SELECT MAX(version) FROM _sqlx_migrations WHERE success = 1").fetchone()[0],
            "entries": con.execute("SELECT COUNT(*) FROM journal_entries").fetchone()[0],
            "lines": con.execute("SELECT COUNT(*) FROM journal_lines").fetchone()[0],
            "integrity": con.execute("PRAGMA integrity_check").fetchone()[0],
            "foreignKeyProblems": len(con.execute("PRAGMA foreign_key_check").fetchall()),
            "balances": balances,
        }
    finally:
        con.close()
        shutil.rmtree(scratch.parent, ignore_errors=True)


def compare_books(name, before, after):
    """Problems if the books changed between `before` and `after`."""
    problems = []
    for key in ("entries", "lines"):
        if before[key] != after[key]:
            problems.append(f"{name}: {key} {before[key]} → {after[key]}")
    changed = [k for k in sorted(set(before["balances"]) | set(after["balances"])) if before["balances"].get(k) != after["balances"].get(k)]
    for k in changed[:10]:
        problems.append(f"{name}: balance of {k} {before['balances'].get(k)} → {after['balances'].get(k)}")
    if len(changed) > 10:
        problems.append(f"{name}: {len(changed) - 10} more balances changed")
    return problems


def cmd_balances(args):
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    summary(f"### Seeded with {args.previous}\n\n| Company | Schema | Entries | Accounts with activity |\n|---|---:|---:|---:|")
    found = companies(args.seed_dir)
    if not found:
        fail(f"the seeder made no company files in {args.seed_dir}")
    for path in found:
        books = read_company(path)
        (out / f"{path.stem}.json").write_text(json.dumps(books, indent=1))
        summary(f"| {path.stem} | {books['schema']} | {books['entries']} | {len({k.split(' [')[0] for k in books['balances']})} |")
    summary()
    finish()


def cmd_upgrade(args, done=True, heading="Upgrade with `--check-file`"):
    work = Path(args.work)
    summary(f"### {heading}\n\n| Company | Schema | Pre-upgrade copy | File Check & Repair | Balances | Result |\n|---|---|---|---|---|---|")
    for source in companies(args.seed_dir):
        name = source.stem
        folder = work / name
        file = copy_company(source, folder)
        result_path = folder / "check.json"
        try:
            process = subprocess.run(
                [args.app, "--check-file", str(file), "--backups-dir", str(folder / "backups"), "--output", str(result_path)],
                capture_output=True, text=True, timeout=CHECK_TIMEOUT,
            )
            code = process.returncode
            if process.stderr.strip():
                print(process.stderr)
        except subprocess.TimeoutExpired:
            fail(f"{name}: --check-file did not finish in {CHECK_TIMEOUT} s")
            summary(f"| {name} | | | | | ❌ timed out |")
            continue
        try:
            result = json.loads(result_path.read_text())
        except (OSError, ValueError) as e:
            fail(f"{name}: --check-file exited {code} without a readable result ({e}); stdout: {process.stdout[-500:]}")
            summary(f"| {name} | | | | | ❌ no result |")
            continue
        problems = [f"{name}: {p}" for p in result["problems"]]
        if code != (0 if result["ok"] else 1):
            problems.append(f"{name}: exit code {code} does not match ok={result['ok']}")
        before = json.loads((Path(args.before) / f"{name}.json").read_text())
        after = read_company(file)
        problems += compare_books(name, before, after)
        if after["schema"] != args.app_schema:
            problems.append(f"{name}: schema {after['schema']} after the check, expected {args.app_schema}")
        for p in problems:
            fail(p)
        checkup = result.get("checkup") or {}
        statuses = ", ".join(f"{c['id']} {c['status'].lower()}" for c in checkup.get("checks", []) if c["status"] != "PASS") or "all pass"
        copy = result.get("preUpgradeBackup")
        summary(
            f"| {name} | {result.get('schemaBefore')} → {result.get('schemaAfter')} | {'✅ ' + copy['fileName'] if copy else ('—' if not result.get('upgraded') else '❌')} "
            f"| {checkup.get('status', '?')} ({statuses}) | {mark(not compare_books(name, before, after))} | {mark(not problems)} |"
        )
    summary()
    if done:
        finish()


# ------------------------------------------------------------------------------------------------
# Launch smoke test
# ------------------------------------------------------------------------------------------------


def data_dir():
    home = Path.home()
    system = platform.system()
    if system == "Darwin":
        return home / "Library/Application Support" / APP_ID
    if system == "Windows":
        return Path(os.environ["APPDATA"]) / APP_ID
    return Path(os.environ.get("XDG_DATA_HOME", home / ".local/share")) / APP_ID


def crash_reports(since):
    """Crash reports for LocalBooks written after `since` (epoch seconds)."""
    system = platform.system()
    found = []
    if system == "Darwin":
        for folder in [Path.home() / "Library/Logs/DiagnosticReports", Path("/Library/Logs/DiagnosticReports")]:
            if folder.is_dir():
                found += [str(p) for p in folder.rglob("*") if p.is_file() and "localbooks" in p.name.lower() and p.stat().st_mtime >= since]
    elif system == "Windows":
        dumps = Path(os.environ["LOCALAPPDATA"]) / "CrashDumps"
        if dumps.is_dir():
            found += [str(p) for p in dumps.iterdir() if "localbooks" in p.name.lower() and p.stat().st_mtime >= since]
        minutes = int((time.time() - since) / 60) + 2
        query = f"*[System[(Provider[@Name='Application Error'] or Provider[@Name='Windows Error Reporting']) and TimeCreated[timediff(@SystemTime) <= {minutes * 60000}]]]"
        events = subprocess.run(["wevtutil", "qe", "Application", f"/q:{query}", "/f:text"], capture_output=True, text=True).stdout
        found += [block.strip()[:400] for block in events.split("Event[") if "localbooks" in block.lower()]
    return found


def quit_app(process):
    """Asks the app to quit (WM_CLOSE on Windows, SIGTERM elsewhere), then forces it."""
    if platform.system() == "Windows":
        subprocess.run(["taskkill", "/PID", str(process.pid), "/T"], capture_output=True)
    else:
        process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=20)
        return "quit"
    except subprocess.TimeoutExpired:
        if platform.system() == "Windows":
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True)
        else:
            process.kill()
        process.wait(timeout=20)
        return "forced"


def cmd_launch(args, done=True, heading=None):
    work = Path(args.work)
    backups = data_dir() / "backups"
    startup_log = data_dir() / "startup-error.log"
    summary(f"### {heading or 'Launch smoke'} ({LAUNCH_SECONDS} s each)\n\n| Company | Running after {LAUNCH_SECONDS} s | Crash reports | Quit | Upgraded by the app | Pre-upgrade copy | Integrity | Balances | Result |\n|---|---|---|---|---|---|---|---|---|")
    for source in companies(args.seed_dir):
        name = source.stem
        file = copy_company(source, work / name)
        before = json.loads((Path(args.before) / f"{name}.json").read_text())
        log_path = work / f"{name}.log"
        problems = []
        started = time.time() - 1
        with open(log_path, "wb") as log:
            process = subprocess.Popen([args.app, str(file)], stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
            time.sleep(LAUNCH_SECONDS)
            running = process.poll() is None
            if not running:
                problems.append(f"{name}: the app exited after launch (code {process.returncode})")
            how = quit_app(process) if running else "—"
        output = log_path.read_text(errors="replace")
        if output.strip():
            print(f"--- {name} app output ---\n{output[-4000:]}")
        if "panicked at" in output:
            problems.append(f"{name}: the app panicked (see the log above)")
        reports = crash_reports(started)
        problems += [f"{name}: crash report {r}" for r in reports]
        if startup_log.exists():
            problems.append(f"{name}: startup-error.log: {startup_log.read_text(errors='replace').strip()[-500:]}")
            startup_log.unlink()

        after = read_company(file)
        upgraded = after["schema"] == args.app_schema
        if not upgraded:
            problems.append(f"{name}: schema {after['schema']} after launch, expected {args.app_schema} (the app did not open and upgrade this file)")
        copies = sorted(backups.glob(f"pre-upgrade-{name}-v{before['schema']}-*.db")) if backups.is_dir() else []
        if before["schema"] < args.app_schema and not copies:
            problems.append(f"{name}: no pre-upgrade copy in {backups}")
        healthy = after["integrity"] == "ok" and after["foreignKeyProblems"] == 0
        if not healthy:
            problems.append(f"{name}: integrity {after['integrity']}, {after['foreignKeyProblems']} broken links")
        changed = compare_books(name, before, after)
        problems += changed
        for p in problems:
            fail(p)
        summary(
            f"| {name} | {mark(running)} | {mark(not reports)} {len(reports) or 'none'} | {how} | {mark(upgraded)} {before['schema']} → {after['schema']} "
            f"| {mark(bool(copies) or before['schema'] >= args.app_schema)} | {mark(healthy)} | {mark(not changed)} | {mark(not problems)} |"
        )
    summary()
    if done:
        finish()


# ------------------------------------------------------------------------------------------------
# Installer upgrade: the previous release installed, then this one over it the way the updater does
# ------------------------------------------------------------------------------------------------
#
# The other steps install this release on a clean machine. Installed copies get it as an update
# instead, and Tauri's installers key some things on the product name (LocalBooks became Tillhouse
# in 0.16: docs/plans/rebrand.md), so this step installs the previous release first, then applies
# this one as tauri-plugin-updater 2.14.1 does, and checks that one copy is left, that it opens the
# seeded companies with unchanged balances, and that the app data folder kept everything in it.
#
#   Windows  the updater runs the NSIS setup with its install mode's arguments and `/UPDATE`
#            (plugins/updater/src/updater.rs `updater_parameters`: `nsis_args()` then "/UPDATE";
#            config.rs `nsis_args`: installMode "passive" is "/P"). It also passes "/R /ARGS …" to
#            restart the app, which only relaunches it, so that is left out here.
#   macOS    the updater unpacks the .app.tar.gz without its top folder and swaps it into the
#            running bundle's own path (updater.rs: `extract_path_from_executable`, then
#            `swap_bundle(&self.extract_path, …)` or `replace_bundle`), so the bundle keeps the
#            previous release's folder name.
#   Linux    `dpkg -i` the new .deb over the old one; `rpm -U` the new .rpm over the old one in a
#            scratch root (no scripts, no dependency checks: the packages' names and Obsoletes are
#            what is checked there).


def product(config_path):
    config = json.loads(Path(config_path).read_text())
    name = config["productName"]
    identifier = config["identifier"]
    manufacturer = config.get("bundle", {}).get("publisher") or identifier.split(".")[1]
    # Tauri names the Linux packages with heck's kebab case of the product name.
    kebab = "".join(("-" + c.lower()) if c.isupper() and i and name[i - 1].islower() else c.lower() for i, c in enumerate(name)).replace(" ", "-")
    return {"name": name, "identifier": identifier, "manufacturer": manufacturer, "package": kebab}


def snapshot(folders):
    """Every file under `folders` (the app data folders), with a marker added to prove it survives."""
    marker = folders[0] / "release-check-marker.txt"
    marker.parent.mkdir(parents=True, exist_ok=True)
    marker.write_text("left by the release check's installer upgrade step\n")
    return {str(p) for folder in folders if folder.is_dir() for p in folder.rglob("*") if p.is_file()}


def check_kept(before, folders, rows):
    after = {str(p) for folder in folders if folder.is_dir() for p in folder.rglob("*") if p.is_file()}
    gone = sorted(before - after)
    if gone:
        fail(f"the upgrade removed {len(gone)} file(s) from the app data folder, such as {gone[:5]}")
    rows.append(("App data folder kept every file", not gone, f"{len(before)} files, {len(gone)} removed"))


def report(rows, heading):
    summary(f"### {heading}\n\n| Check | Result | Details |\n|---|---|---|")
    for check, ok, details in rows:
        summary(f"| {check} | {mark(ok)} | {details} |")
    summary()


def expect(rows, check, ok, details, message=None):
    rows.append((check, ok, details))
    if not ok:
        fail(message or f"{check}: {details}")


def run_new_app(args, exe, label):
    """The upgraded app opens a copy of each seeded company: `--check-file`, as the Upgrade step."""
    checked = argparse.Namespace(app=str(exe), seed_dir=args.seed_dir, before=args.before, work=str(Path(args.work) / label), app_schema=args.app_schema)
    cmd_upgrade(checked, done=False, heading=f"Upgraded install ({label}): `--check-file`")
    return checked


def upgrade_windows(args, old, new):
    import winreg

    uninstall = r"Software\Microsoft\Windows\CurrentVersion\Uninstall"

    def read(path, value=""):
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as key:
                return winreg.QueryValueEx(key, value)[0]
        except OSError:
            return None

    def entries():
        names = []
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, uninstall) as key:
            for i in range(winreg.QueryInfoKey(key)[0]):
                sub = winreg.EnumKey(key, i)
                if sub in (old["name"], new["name"]) or read(f"{uninstall}\\{sub}", "DisplayName") in (old["name"], new["name"]):
                    names.append(sub)
        return sorted(set(names))

    def exe_in(folder):
        found = [p for p in Path(folder).glob("*.exe") if not p.name.lower().startswith("uninstall")] if folder and Path(folder).is_dir() else []
        return found[0] if found else None

    # A clean machine: remove the copy the Install step put here (the uninstaller run in place,
    # `_?=`, so it finishes before this goes on; run silently it keeps the app data).
    for product_ in (new, old):
        folder = read(f"Software\\{product_['manufacturer']}\\{product_['name']}")
        if folder and (Path(folder) / "uninstall.exe").exists():
            subprocess.run([str(Path(folder) / "uninstall.exe"), "/S", f"_?={folder}"], check=False, timeout=INSTALL_TIMEOUT)
            shutil.rmtree(folder, ignore_errors=True)
        for key in (f"{uninstall}\\{product_['name']}", f"Software\\{product_['manufacturer']}\\{product_['name']}"):
            subprocess.run(["reg", "delete", f"HKCU\\{key}", "/f"], check=False, capture_output=True)
    if entries():
        raise SystemExit(f"could not remove the earlier installs before the upgrade: {entries()}")

    data = [Path(os.environ["APPDATA"]) / new["identifier"], Path(os.environ["LOCALAPPDATA"]) / new["identifier"]]
    kept = snapshot(data)
    rows = []

    subprocess.run([str(only(args.previous_dir, "-setup.exe")), "/S"], check=True, timeout=INSTALL_TIMEOUT)
    old_folder = read(f"Software\\{old['manufacturer']}\\{old['name']}")
    expect(rows, f"{old['name']} {args.previous} installed", bool(exe_in(old_folder)) and entries() == [old["name"]], f"{old_folder}; uninstall entries {entries()}")

    setup = only(args.release_dir, "-setup.exe")
    subprocess.run([str(setup), "/P", "/UPDATE"], check=True, timeout=INSTALL_TIMEOUT)
    new_folder = read(f"Software\\{new['manufacturer']}\\{new['name']}")
    exe = exe_in(new_folder)
    expect(rows, f"Updated with `{setup.name} /P /UPDATE`", bool(exe), f"{exe or 'no executable'} in {new_folder}")
    expect(rows, "One uninstall entry", entries() == [new["name"]], f"{entries()}", f"uninstall entries after the upgrade are {entries()}, expected only {new['name']}")
    start_menu = Path(os.environ["APPDATA"]) / "Microsoft/Windows/Start Menu/Programs"
    if old["name"] != new["name"]:
        left = sorted(p.name for p in Path(old_folder).iterdir()) if old_folder and Path(old_folder).is_dir() else []
        expect(rows, f"{old['name']} install folder removed", not left, f"{old_folder}: {', '.join(left) if left else 'gone'}")
        expect(rows, f"{old['name']} registry keys removed", read(f"Software\\{old['manufacturer']}\\{old['name']}") is None, f"Software\\{old['manufacturer']}\\{old['name']}")
        expect(rows, f"{old['name']} Start menu shortcut removed", not (start_menu / f"{old['name']}.lnk").exists(), str(start_menu / f"{old['name']}.lnk"))
    expect(rows, f"{new['name']} Start menu shortcut", (start_menu / f"{new['name']}.lnk").exists(), str(start_menu / f"{new['name']}.lnk"))
    check_kept(kept, data, rows)
    report(rows, f"Installer upgrade: {args.previous} → this release (Windows, NSIS)")
    if exe:
        run_new_app(args, exe, "windows-nsis")


def rpm_in(root):
    """rpm acting on the scratch root only. Ubuntu's rpm keeps its database under $HOME unless told
    otherwise (and sudo changes $HOME), so every call names the same database inside the root."""
    return ["sudo", "rpm", "--root", str(root), "--dbpath", "/var/lib/rpm"]


def upgrade_linux(args, old, new):
    data = [Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / new["identifier"], Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / new["identifier"]]
    kept = snapshot(data)

    def status(package, root=None):
        if root:
            return subprocess.run([*rpm_in(root), "-q", package], capture_output=True, text=True).returncode == 0
        out = subprocess.run(["dpkg-query", "-W", "-f=${Status}", package], capture_output=True, text=True)
        return out.returncode == 0 and out.stdout.strip() == "install ok installed"

    def binary(files):
        found = [f for f in files if "/usr/bin/" in f and Path(f).is_file()]
        return Path(found[0]) if found else None

    # .deb: remove the copy the Install step put here, install the previous package, then the new one.
    rows = []
    subprocess.run(["sudo", "dpkg", "-r", new["package"], old["package"]], check=False, capture_output=True)
    subprocess.run(["sudo", "dpkg", "-i", str(only(args.previous_dir, ".deb").resolve())], check=True, stdout=subprocess.DEVNULL)
    expect(rows, f"{old['package']} ({args.previous}) installed", status(old["package"]), "dpkg -i")
    deb = only(args.release_dir, ".deb")
    installed = subprocess.run(["sudo", "dpkg", "-i", str(deb.resolve())], capture_output=True, text=True)
    expect(rows, f"`dpkg -i {deb.name}` over it", installed.returncode == 0, (installed.stderr or installed.stdout).strip()[-300:].replace("\n", " ") or "ok")
    expect(rows, f"{new['package']} installed", status(new["package"]), "dpkg-query")
    if old["package"] != new["package"]:
        expect(rows, f"{old['package']} removed", not status(old["package"]), "dpkg-query", f"{old['package']} is still installed after installing {new['package']}")
    files = subprocess.run(["dpkg", "-L", new["package"]], capture_output=True, text=True).stdout.splitlines()
    deb_exe = binary(files)
    expect(rows, "Executable", bool(deb_exe), str(deb_exe))
    check_kept(kept, data, rows)
    report(rows, f"Installer upgrade: {args.previous} → this release (Linux, .deb)")
    if deb_exe:
        run_new_app(args, deb_exe, "linux-deb")

    # .rpm: in a scratch root, so it doesn't meet the .deb's files.
    rows = []
    root = Path(args.work) / "rpm-root"
    shutil.rmtree(root, ignore_errors=True)
    root.mkdir(parents=True)
    subprocess.run([*rpm_in(root), "--initdb"], check=True)
    rpm_flags = ["--nodeps", "--noscripts", "--nosignature"]
    installed = subprocess.run([*rpm_in(root), "-i", *rpm_flags, str(only(args.previous_dir, ".rpm").resolve())], capture_output=True, text=True)
    expect(rows, f"{old['package']} ({args.previous}) installed", installed.returncode == 0 and status(old["package"], root), f"rpm -i: {(installed.stderr or installed.stdout).strip()[-300:].replace(chr(10), ' ') or 'ok'}")
    rpm = only(args.release_dir, ".rpm")
    upgraded = subprocess.run([*rpm_in(root), "-U", *rpm_flags, str(rpm.resolve())], capture_output=True, text=True)
    expect(rows, f"`rpm -U {rpm.name}` over it", upgraded.returncode == 0, (upgraded.stderr or upgraded.stdout).strip()[-300:].replace("\n", " ") or "ok")
    expect(rows, f"{new['package']} installed", status(new["package"], root), "rpm -q")
    if old["package"] != new["package"]:
        expect(rows, f"{old['package']} removed (Obsoletes)", not status(old["package"], root), "rpm -q", f"{old['package']} is still installed after rpm -U {rpm.name}")
    listed = subprocess.run([*rpm_in(root), "-ql", new["package"]], capture_output=True, text=True).stdout.splitlines()
    rpm_exe = binary([str(root) + f for f in listed])
    expect(rows, "Executable", bool(rpm_exe), str(rpm_exe))
    report(rows, f"Installer upgrade: {args.previous} → this release (Linux, .rpm)")
    if rpm_exe:
        run_new_app(args, rpm_exe, "linux-rpm")


def upgrade_macos(args, old, new):
    data = [Path.home() / "Library/Application Support" / new["identifier"]]
    kept = snapshot(data)
    rows = []
    applications = Path(args.work) / "Applications"
    shutil.rmtree(applications, ignore_errors=True)
    applications.mkdir(parents=True)
    # The previous release as installed: its update bundle holds the .app as its top folder.
    subprocess.run(["tar", "-xzf", str(only(args.previous_dir, ".app.tar.gz")), "-C", str(applications)], check=True)
    bundles = sorted(applications.glob("*.app"))
    expect(rows, f"{old['name']} ({args.previous}) in Applications", len(bundles) == 1, ", ".join(b.name for b in bundles))
    if len(bundles) != 1:
        report(rows, f"Installer upgrade: {args.previous} → this release (macOS)")
        return
    bundle = bundles[0]
    # What the updater does: unpack without the top folder into a temp dir on the same volume,
    # then move it into the current bundle's path (the rename fallback of `swap_bundle`).
    staged = Path(tempfile.mkdtemp(prefix="tauri_updated_app", dir=args.work))
    archive = only(args.release_dir, ".app.tar.gz")
    subprocess.run(["tar", "-xzf", str(archive), "-C", str(staged), "--strip-components", "1"], check=True)
    os.chmod(staged, 0o755)
    backup = Path(tempfile.mkdtemp(prefix="tauri_current_app", dir=args.work)) / "current_app"
    os.rename(bundle, backup)
    os.rename(staged, bundle)
    info = bundle / "Contents/Info.plist"

    def plist(key):
        return subprocess.run(["plutil", "-extract", key, "raw", str(info)], capture_output=True, text=True).stdout.strip()

    expect(rows, f"Updated with `{archive.name}` in place", info.exists(), f"bundle kept its path: {bundle.name}")
    expect(rows, "Shows the new name", plist("CFBundleName") == new["name"], f"CFBundleName {plist('CFBundleName')!r}")
    expect(rows, "Same identifier", plist("CFBundleIdentifier") == new["identifier"], plist("CFBundleIdentifier"))
    exe = bundle / "Contents/MacOS" / plist("CFBundleExecutable")
    expect(rows, "Executable", exe.is_file(), str(exe.relative_to(applications)))
    check_kept(kept, data, rows)
    report(rows, f"Installer upgrade: {args.previous} → this release (macOS)")
    if exe.is_file():
        checked = run_new_app(args, exe, "macos")
        checked.work = str(Path(args.work) / "macos-launch")
        cmd_launch(checked, done=False, heading="Upgraded install (macOS): launch")
        check_old_name_renames(applications, bundle)


def check_old_name_renames(applications, bundle):
    """A copy still named LocalBooks.app (updated in place from 0.15 or older) renames itself to
    Tillhouse.app on its next launch (src-tauri/src/mac_app_name.rs). The previous release already
    has the new name, so this puts the new build at the old name and launches it."""
    rows = []
    folder = applications / "old-name"
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir()
    old_copy = folder / "LocalBooks.app"
    subprocess.run(["cp", "-R", str(bundle), str(old_copy)], check=True)
    renamed = folder / "Tillhouse.app"
    exe = old_copy / "Contents/MacOS" / next((old_copy / "Contents/MacOS").iterdir()).name
    proc = subprocess.Popen([str(exe)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    deadline = time.time() + 30
    while time.time() < deadline and not renamed.is_dir():
        time.sleep(1)
    alive = proc.poll() is None
    proc.kill()
    proc.wait()
    expect(rows, "LocalBooks.app renames itself to Tillhouse.app", renamed.is_dir() and not old_copy.exists(), ", ".join(sorted(p.name for p in folder.iterdir())))
    expect(rows, "Keeps running after the rename", alive, "restarted from Tillhouse.app" if alive else "exited")
    report(rows, "Old app name (macOS)")


def cmd_installer_upgrade(args):
    old, new = product(args.previous_config), product(args.config)
    summary(f"Upgrading an installed **{old['name']} {args.previous}** to **{new['name']}** as the updater does.\n")
    {"windows": upgrade_windows, "linux": upgrade_linux, "darwin": upgrade_macos}[args.platform](args, old, new)
    finish()


def main():
    # Windows consoles default to a code page without ✅/❌.
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("artifacts")
    p.add_argument("--release-dir", required=True)
    p.add_argument("--config", required=True, help="the release's tauri.conf.json")
    p.add_argument("--previous-config", help="the previous release's tauri.conf.json")
    p.add_argument("--platform", choices=INSTALLERS, required=True)
    p.add_argument("--version", required=True)
    p = sub.add_parser("install")
    p.add_argument("--release-dir", required=True)
    p.add_argument("--platform", choices=INSTALLERS, required=True)
    p.add_argument("--dest", default=str(Path(tempfile.gettempdir()) / "lb-apps"))
    p = sub.add_parser("codesign")
    p.add_argument("--app", required=True)
    p = sub.add_parser("balances")
    p.add_argument("--seed-dir", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--previous", required=True)
    for name in ("upgrade", "launch"):
        p = sub.add_parser(name)
        p.add_argument("--app", required=True)
        p.add_argument("--seed-dir", required=True)
        p.add_argument("--before", required=True)
        p.add_argument("--work", required=True)
        p.add_argument("--app-schema", type=int, required=True)
    p = sub.add_parser("installer-upgrade")
    p.add_argument("--platform", choices=INSTALLERS, required=True)
    p.add_argument("--release-dir", required=True, help="this release's installers")
    p.add_argument("--previous-dir", required=True, help="the previous release's installers")
    p.add_argument("--config", required=True, help="this release's tauri.conf.json")
    p.add_argument("--previous-config", required=True, help="the previous release's tauri.conf.json")
    p.add_argument("--previous", required=True, help="the previous release's tag")
    p.add_argument("--seed-dir", required=True)
    p.add_argument("--before", required=True)
    p.add_argument("--work", required=True)
    p.add_argument("--app-schema", type=int, required=True)
    args = parser.parse_args()
    {
        "artifacts": cmd_artifacts,
        "install": cmd_install,
        "codesign": cmd_codesign,
        "balances": cmd_balances,
        "upgrade": cmd_upgrade,
        "launch": cmd_launch,
        "installer-upgrade": cmd_installer_upgrade,
    }[args.command](args)


if __name__ == "__main__":
    main()
