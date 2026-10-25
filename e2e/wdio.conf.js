// End-to-end tests: WebdriverIO drives the real debug build through tauri-driver (Windows and Linux;
// macOS has no WebDriver for its webview). See docs/testing/end-to-end.md.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

import { APP, SCREENSHOTS, keepOwnConfig, prepareCompany, seedCompanies, waitForAppExit } from './lib/app.js';

/** The seeded company each spec opens a fresh copy of. */
const COMPANIES = {
  'users-from-scratch': 'No Users.localbooks',
  'seeded-users': 'Edge Cases.localbooks',
  'taxed-invoice-edit': 'Edge Cases.localbooks',
};

const TAURI_DRIVER = process.env.TAURI_DRIVER || path.join(os.homedir(), '.cargo', 'bin', process.platform === 'win32' ? 'tauri-driver.exe' : 'tauri-driver');
const DRIVER_PORT = 4444;

let tauriDriver;
let configDir;
let restoreOwnConfig = () => {};

/** Resolves once something accepts connections on `port`. */
async function waitForPort(port, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const open = await new Promise((resolve) => {
      const socket = net.connect(port, '127.0.0.1', () => (socket.end(), resolve(true)));
      socket.on('error', () => resolve(false));
    });
    if (open) return;
    if (Date.now() > deadline) throw new Error(`tauri-driver did not start on port ${port}`);
    await new Promise((r) => setTimeout(r, 100));
  }
}

export const config = {
  runner: 'local',
  hostname: '127.0.0.1',
  port: DRIVER_PORT,
  specs: ['./specs/*.e2e.js'],
  // One app at a time: the specs share one tauri-driver port and, on Windows, one settings folder.
  maxInstances: 1,
  capabilities: [{ 'tauri:options': { application: APP }, 'wdio:enforceWebDriverClassic': true }],
  logLevel: 'warn',
  waitforTimeout: 15_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 1,
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: { ui: 'bdd', timeout: 120_000 },

  onPrepare() {
    if (!fs.existsSync(APP)) throw new Error(`${APP} is missing: run \`npx tauri build --debug --no-bundle\` first`);
    restoreOwnConfig = keepOwnConfig();
    if (!process.env.LOCALBOOKS_E2E_KEEP_SEEDS) seedCompanies();
    fs.rmSync(SCREENSHOTS, { recursive: true, force: true });
  },

  // Each spec file is its own session: a fresh company copy, a fresh tauri-driver, a fresh app.
  async beforeSession(_config, _capabilities, specs) {
    const name = path.basename(specs[0], '.e2e.js');
    if (!COMPANIES[name]) throw new Error(`no company is set for ${name} in wdio.conf.js`);
    const prepared = prepareCompany(name, COMPANIES[name]);
    configDir = prepared.configDir;
    const args = process.env.NATIVE_DRIVER ? ['--native-driver', process.env.NATIVE_DRIVER] : [];
    tauriDriver = spawn(TAURI_DRIVER, args, {
      env: { ...process.env, ...prepared.env, WEBKIT_DISABLE_DMABUF_RENDERER: '1' },
      stdio: ['ignore', 'inherit', 'inherit'],
    });
    await waitForPort(DRIVER_PORT);
  },

  async afterTest(test, _context, { passed }) {
    if (passed) return;
    fs.mkdirSync(SCREENSHOTS, { recursive: true });
    const file = `${test.parent} -- ${test.title}`.replace(/[^\w.-]+/g, '_').slice(0, 150);
    await browser.saveScreenshot(path.join(SCREENSHOTS, `${file}.png`)).catch(() => {});
  },

  async afterSession() {
    await waitForAppExit(configDir);
    if (tauriDriver && tauriDriver.exitCode === null) {
      const exited = new Promise((resolve) => tauriDriver.once('exit', resolve));
      tauriDriver.kill();
      await exited;
    }
  },

  onComplete() {
    restoreOwnConfig();
  },
};
