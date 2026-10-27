import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

/** Where each reminder category's "Go" link leads: [view, action]. */
const GO = {
  OVERDUE_INVOICES: ['sales/invoices', null, 'Invoices'],
  BILLS_DUE: ['expenses/bills', 'pay-bills', 'Pay Bills'],
  TODOS: ['accounting/todos', null, 'To Do List'],
  RECURRING: ['accounting/recurring', null, 'Recurring Transactions'],
  EMAIL_QUEUE: ['sales/send-forms', null, 'Send Forms'],
  TAX_DATES: ['settings', 'preferences', 'Tax calendar'],
  BACKUP: ['settings', 'data', 'Back up'],
};

/** The categories in the order they show, with what their days mean. */
const CATEGORIES = [
  { key: 'overdueInvoices', label: 'Overdue invoices', days: 'Days past due before an invoice shows' },
  { key: 'billsDue', label: 'Bills to pay', days: 'Days ahead to look for bills due' },
  { key: 'todos', label: 'To-dos due', days: 'Days ahead to look for to-dos due' },
  { key: 'recurring', label: 'Recurring transactions to enter', days: 'Days ahead to look for scheduled dates' },
  { key: 'emailQueue', label: 'Forms to email (Email later)' },
  { key: 'taxDates', label: 'Tax dates', days: 'Days ahead to look for federal due dates' },
  { key: 'backup', label: 'Backup', days: 'Days since the last backup before it is due' },
];

function ReminderGroup({ group, open, onToggle, navigate }) {
  const [view, action, goLabel] = GO[group.key] ?? [];
  return (
    <li className="border-b border-qb-border last:border-b-0">
      <div className="flex items-center gap-1 px-2 py-1 text-sm">
        <button type="button" className="flex min-w-0 flex-1 items-center gap-1 text-left font-semibold hover:underline" onClick={onToggle} aria-expanded={open}>
          <span className="w-3 flex-none text-qb-ink-muted">{open ? '▾' : '▸'}</span>
          <span className="truncate">{group.label}</span>
          <span className="flex-none rounded-sm bg-qb-green-tint px-1 text-xs text-qb-green-active">{group.items.length}</span>
        </button>
        {group.totalCents != null && <span className="num flex-none font-semibold">{formatCents(group.totalCents)}</span>}
        {view && (
          <button type="button" className="flex-none text-xs text-qb-link hover:underline" onClick={() => navigate(view, action)}>
            {goLabel}
          </button>
        )}
      </div>
      {open && (
        <ul className="pb-1 pl-6 pr-2 text-xs">
          {group.items.map((item, i) => (
            <li key={`${item.id ?? 'x'}-${i}`} className="flex items-baseline gap-2 py-0.5">
              {item.entryId ? (
                <button type="button" className="min-w-0 flex-1 truncate text-left text-qb-link hover:underline" title={item.label} onClick={() => navigate('accounting/journal', 'open-entry', { entryId: item.entryId })}>
                  {item.label}
                </button>
              ) : (
                <span className="min-w-0 flex-1 truncate" title={item.label}>{item.label}</span>
              )}
              <span className="flex-none text-qb-ink-muted">{item.detail}</span>
              {item.date && <span className="w-[72px] flex-none text-right text-qb-ink-muted">{formatDate(item.date)}</span>}
              {item.amountCents != null && <span className="num w-[84px] flex-none">{formatCents(item.amountCents)}</span>}
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

/** The reminders as a list of expandable categories; `navigate` opens where to deal with each. */
export function ReminderList({ navigate }) {
  const { data, error } = useLedgerQuery(api.getReminders);
  const [openKeys, setOpenKeys] = useState(() => new Set());
  if (error) return <Notice kind="error" text={`Could not load reminders: ${errorText(error)}`} />;
  if (!data) return <div className="px-2 py-1 text-sm text-qb-ink-muted">Loading…</div>;
  if (data.length === 0) return <div className="px-2 py-2 text-sm text-qb-ink-muted">✓ Nothing needs attention today.</div>;
  const toggle = (key) =>
    setOpenKeys((s) => {
      const next = new Set(s);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  return (
    <ul>
      {data.map((g) => (
        <ReminderGroup key={g.key} group={g} open={openKeys.has(g.key)} onToggle={() => toggle(g.key)} navigate={navigate} />
      ))}
    </ul>
  );
}

/** Which reminders show and how far ahead they look: the signed-in user's own choice. */
export function ReminderSettingsDialog({ onClose }) {
  const [settings, setSettings] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api.getReminderSettings().then(setSettings, (err) => setError(errorText(err)));
  }, []);
  const setCategory = (key, patch) => setSettings((s) => ({ ...s, [key]: { ...s[key], ...patch } }));
  const submit = async (e) => {
    e.preventDefault();
    setBusy(true);
    try {
      await api.saveReminderSettings(settings);
      onClose();
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };
  return (
    <Dialog title="Reminder preferences" onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-sm">
        {settings && (
          <>
            <p className="text-xs text-qb-ink-muted">These are your own: each user chooses their reminders.</p>
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head w-[52px]">Show</th>
                  <th className="qb-grid-head">Reminder</th>
                  <th className="qb-grid-head w-[90px]">Days</th>
                </tr>
              </thead>
              <tbody>
                {CATEGORIES.map((c) => (
                  <tr key={c.key} className="qb-grid-row">
                    <td className="qb-grid-cell text-center">
                      <input type="checkbox" className="accent-qb-green" checked={settings[c.key].show} onChange={(e) => setCategory(c.key, { show: e.target.checked })} aria-label={`Show ${c.label}`} />
                    </td>
                    <td className="qb-grid-cell" title={c.days}>
                      {c.label}
                      {c.days && <span className="block text-2xs text-qb-ink-muted">{c.days}</span>}
                    </td>
                    <td className="qb-grid-cell-edit">
                      {c.days && (
                        <input
                          type="number"
                          min={c.key === 'backup' ? 1 : 0}
                          max={365}
                          value={settings[c.key].days}
                          onChange={(e) => setCategory(c.key, { days: e.target.value === '' ? 0 : Number(e.target.value) })}
                          aria-label={c.days}
                        />
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <label className="flex items-center gap-1.5">
              <input type="checkbox" className="accent-qb-green" checked={settings.showAtStartup} onChange={(e) => setSettings((s) => ({ ...s, showAtStartup: e.target.checked }))} />
              Show Reminders when the company opens
            </label>
          </>
        )}
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy || !settings}>Save</button>
        </div>
      </form>
    </Dialog>
  );
}

/** The Dashboard's Reminders panel. */
export default function RemindersPanel({ navigate }) {
  const [customizing, setCustomizing] = useState(false);
  return (
    <section className="qb-panel flex min-h-0 flex-col">
      {customizing && <ReminderSettingsDialog onClose={() => setCustomizing(false)} />}
      <div className="flex h-row-lg flex-none items-center justify-between border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Reminders</h2>
        <button type="button" className="text-xs text-qb-link hover:underline" onClick={() => setCustomizing(true)}>
          Customize
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <ReminderList navigate={navigate} />
      </div>
    </section>
  );
}

const SHOWN_KEY = 'localbooks.remindersShown';

/**
 * The Reminders window at startup, when the signed-in user chose it: once per company file and
 * user in each run of the app. `fileKey` names the open file.
 */
export function StartupReminders({ fileKey, userKey, navigate }) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!fileKey) return undefined;
    const key = `${fileKey}|${userKey ?? ''}`;
    try {
      if (sessionStorage.getItem(SHOWN_KEY) === key) return undefined;
    } catch {
      // Storage unavailable: show it once for this window.
    }
    let live = true;
    api.getReminderSettings().then(
      (s) => {
        if (!live || !s.showAtStartup) return;
        try {
          sessionStorage.setItem(SHOWN_KEY, key);
        } catch {
          // Only a convenience.
        }
        setOpen(true);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [fileKey, userKey]);
  if (!open) return null;
  const go = (...args) => {
    setOpen(false);
    navigate(...args);
  };
  return (
    <Dialog title="Reminders" onClose={() => setOpen(false)}>
      <div className="max-h-[60vh] overflow-auto scrollbar-thin">
        <ReminderList navigate={go} />
      </div>
      <div className="flex justify-end gap-2 border-t border-qb-border px-3 py-2">
        <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" onClick={() => setOpen(false)}>Close</button>
      </div>
    </Dialog>
  );
}
