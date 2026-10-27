import { useEffect, useState } from 'react';
import { api } from '../lib/api.js';

const SDK_URL = 'https://developer.intuit.com/app/developer/qbdesktop/docs/get-started/download-and-install-the-sdk';

function MacSteps({ status }) {
  const app = status.running ?? status.installed[0] ?? 'QuickBooks';
  return (
    <>
      <p className="text-xs text-qb-ink-muted">
        {status.running
          ? `${status.running} is running.`
          : status.installed.length
            ? `Found ${status.installed.join(', ')}, not running.`
            : 'QuickBooks Desktop for Mac was not found in your Applications folder.'}
      </p>
      <ol className="list-decimal space-y-1 pl-5 text-sm">
        <li>Open {app} and your company file.</li>
        <li>
          Choose <strong>Reports › Accountant &amp; Taxes › Journal</strong>, set the dates to <strong>All</strong>, and leave the report as the front window. LocalBooks reads the
          ledger from it; the lists come straight from QuickBooks.
        </li>
        <li>
          Click <strong>Export from QuickBooks</strong>. The first time, macOS asks whether <strong>LocalBooks</strong> may control <strong>{app}</strong>: click{' '}
          <strong>OK</strong>. That permission lets LocalBooks ask {app} for your data; LocalBooks only sends read-only requests and changes nothing in QuickBooks. If you clicked
          Don’t Allow, turn LocalBooks’ switch for {app} back on in System Settings › Privacy &amp; Security › Automation.
        </li>
      </ol>
    </>
  );
}

function WindowsSteps({ status }) {
  return (
    <>
      <p className="text-xs text-qb-ink-muted">
        {status.sdkInstalled ? 'The QuickBooks SDK is installed.' : 'The QuickBooks SDK is not installed. LocalBooks talks to QuickBooks through it.'}
      </p>
      <ol className="list-decimal space-y-1 pl-5 text-sm">
        <li className={status.sdkInstalled ? 'text-qb-ink-muted' : ''}>
          Install Intuit’s free <strong>QuickBooks Desktop SDK</strong>, from{' '}
          <span className="select-all break-all font-mono text-xs">{SDK_URL}</span>
        </li>
        <li>Open QuickBooks and your company file, signed in as the Admin user (needed the first time only).</li>
        <li>
          Click <strong>Export from QuickBooks</strong>. QuickBooks asks whether to let <strong>LocalBooks</strong> read this company file: choose{' '}
          <strong>Yes, whenever this QuickBooks company file is open</strong>, then Continue. LocalBooks only sends read-only requests and changes nothing in QuickBooks.
        </li>
      </ol>
    </>
  );
}

/** Exports the company open in the QuickBooks on this computer, through Intuit's own interfaces. */
export default function InstalledQuickBooks({ busy, onExport }) {
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState(null);

  // Only looks; nothing is asked of QuickBooks until Export, so no permission prompt appears here.
  useEffect(() => {
    if (!open) return;
    let stale = false;
    api
      .inspectInstalledQuickbooks()
      .then((s) => !stale && setStatus(s))
      .catch(() => !stale && setStatus({ platform: null }));
    return () => {
      stale = true;
    };
  }, [open]);

  const supported = status?.platform === 'mac' || status?.platform === 'windows';
  return (
    <div className="border-t border-qb-border px-3 py-2">
      <button type="button" className="flex w-full items-center gap-2 text-left text-sm font-semibold" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span aria-hidden="true">{open ? '▾' : '▸'}</span>
        QuickBooks still installed? Export from my installed QuickBooks
        <span className="rounded-sm bg-qb-canvas px-1.5 py-0.5 text-2xs font-semibold uppercase text-qb-ink-muted">Preview</span>
      </button>
      {open && (
        <div className="mt-2 flex flex-col gap-2">
          <p className="text-xs text-qb-ink-muted">
            LocalBooks asks the QuickBooks running on this computer for the open company’s chart of accounts, customers, vendors, items and Journal, through Intuit’s
            own interfaces, then reads them like report exports. Nothing is added to your books until you post. <strong>Preview:</strong> this was built from Intuit’s
            documentation and has not yet been tried against a running QuickBooks. If it fails, export the reports from QuickBooks and choose them above.
          </p>
          {!status ? (
            <p className="text-xs text-qb-ink-muted">Looking for QuickBooks…</p>
          ) : status.platform === 'mac' ? (
            <MacSteps status={status} />
          ) : status.platform === 'windows' ? (
            <WindowsSteps status={status} />
          ) : (
            <p className="text-sm">QuickBooks Desktop runs on macOS and Windows. On this computer, export its reports and choose the files above.</p>
          )}
          {supported && (
            <div>
              <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4 text-md" disabled={busy} onClick={onExport}>
                Export from QuickBooks
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
