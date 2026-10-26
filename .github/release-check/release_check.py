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
        roots = [Path(os.environ["LOCALAPPDATA"]) / "LocalBooks", Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "LocalBooks"]
        candidates = [p for root in roots if root.is_dir() for p in root.glob("*.exe") if not p.name.lower().startswith("uninstall")]
        if not candidates:
            raise SystemExit(f"the installer finished but no LocalBooks executable was found in {roots}")
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


def cmd_upgrade(args):
    work = Path(args.work)
    summary("### Upgrade with `--check-file`\n\n| Company | Schema | Pre-upgrade copy | File Dentist | Balances | Result |\n|---|---|---|---|---|---|")
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


def cmd_launch(args):
    work = Path(args.work)
    backups = data_dir() / "backups"
    startup_log = data_dir() / "startup-error.log"
    summary(f"### Launch smoke ({LAUNCH_SECONDS} s each)\n\n| Company | Running after {LAUNCH_SECONDS} s | Crash reports | Quit | Upgraded by the app | Pre-upgrade copy | Integrity | Balances | Result |\n|---|---|---|---|---|---|---|---|---|")
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
    finish()


def main():
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
    args = parser.parse_args()
    {
        "artifacts": cmd_artifacts,
        "install": cmd_install,
        "codesign": cmd_codesign,
        "balances": cmd_balances,
        "upgrade": cmd_upgrade,
        "launch": cmd_launch,
    }[args.command](args)


if __name__ == "__main__":
    main()
