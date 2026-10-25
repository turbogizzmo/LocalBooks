// Paths for an end-to-end run, the seeded test companies, and the fresh company file each spec opens.
//
// A spec opens a copy of one seeded company. The copy is named `localbooks.db` in a folder of its
// own, and the debug build's `config.json` points `database_directory` at that folder, so the app
// opens it at launch exactly as it would a moved company file. (A launch argument would be simpler,
// but on Windows msedgedriver hands `args` to the WebView2 browser process, not to the app.)
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const E2E_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const REPO = path.resolve(E2E_DIR, '..');
export const WORK = path.join(E2E_DIR, '.work');
export const SEEDS = path.join(WORK, 'seeds');
export const SCREENSHOTS = path.join(WORK, 'screenshots');
export const APP = path.join(REPO, 'src-tauri', 'target', 'debug', process.platform === 'win32' ? 'localbooks.exe' : 'localbooks');

const IDENTIFIER = 'com.localbooks.desktop';
const IS_LINUX = process.platform === 'linux';

/** The debug build's settings folder (`data_dirs` in src-tauri/src/main.rs), under `env`. */
export function appConfigDir(env = process.env) {
  const base =
    process.platform === 'win32'
      ? env.APPDATA
      : process.platform === 'darwin'
        ? path.join(os.homedir(), 'Library', 'Application Support')
        : env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
  return path.join(base, IDENTIFIER, 'dev-builds');
}

/** Writes fresh test companies into `.work/seeds` with the test-only seeder (needs the built screens). */
export function seedCompanies() {
  fs.rmSync(SEEDS, { recursive: true, force: true });
  const args = ['test', '--features', 'tauri/custom-protocol', '--bin', 'localbooks', '--', '--ignored', '--exact', 'test_companies::seed_test_companies'];
  const result = spawnSync('cargo', args, {
    cwd: path.join(REPO, 'src-tauri'),
    env: { ...process.env, LOCALBOOKS_SEED_DIR: SEEDS, LOCALBOOKS_SEED_SKIP_LARGE: '1' },
    stdio: 'inherit',
  });
  if (result.status !== 0) throw new Error(`seeding the test companies failed (exit ${result.status ?? result.error})`);
}

/** A value from the seeder's README.txt, e.g. `seedNote(/recovery key: (\S+)/)`. */
export function seedNote(pattern) {
  const match = fs.readFileSync(path.join(SEEDS, 'README.txt'), 'utf8').match(pattern);
  if (!match) throw new Error(`README.txt has no ${pattern}`);
  return match[1];
}

/**
 * Copies `seedFile` into a new folder for `runName` and points the app at it. Returns the
 * environment the app must start with: on Linux every spec also gets its own settings and data
 * folders, so nothing (audit anchors, the running-instance record) carries over between specs.
 */
export function prepareCompany(runName, seedFile) {
  const run = path.join(WORK, 'runs', runName);
  fs.rmSync(run, { recursive: true, force: true });
  const company = path.join(run, 'company');
  fs.mkdirSync(company, { recursive: true });
  fs.copyFileSync(path.join(SEEDS, seedFile), path.join(company, 'localbooks.db'));
  const env = IS_LINUX ? { XDG_CONFIG_HOME: path.join(run, 'config'), XDG_DATA_HOME: path.join(run, 'data') } : {};
  const configDir = appConfigDir({ ...process.env, ...env });
  fs.mkdirSync(configDir, { recursive: true });
  fs.writeFileSync(path.join(configDir, 'config.json'), JSON.stringify({ database_directory: company }, null, 2));
  return { env, configDir, companyFile: path.join(company, 'localbooks.db') };
}

const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

/**
 * Waits for the LocalBooks this run started (the pid in its `instance.json`) to quit, ending it
 * after `timeoutMs`, so the next spec's launch never hands its company file to a leftover window.
 */
export async function waitForAppExit(configDir, timeoutMs = 10_000) {
  const record = path.join(configDir, 'instance.json');
  let pid;
  try {
    pid = JSON.parse(fs.readFileSync(record, 'utf8')).pid;
  } catch {
    return;
  }
  const deadline = Date.now() + timeoutMs;
  while (alive(pid) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
  if (alive(pid)) process.kill(pid);
  fs.rmSync(record, { force: true });
}

/** Saves the developer's own debug-build config.json (Windows and macOS share it with these runs). */
export function keepOwnConfig() {
  if (IS_LINUX) return () => {};
  const file = path.join(appConfigDir(), 'config.json');
  const saved = fs.existsSync(file) ? fs.readFileSync(file) : null;
  return () => (saved ? fs.writeFileSync(file, saved) : fs.rmSync(file, { force: true }));
}
