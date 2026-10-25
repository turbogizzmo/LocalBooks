import { useState } from 'react';
import { save } from '@tauri-apps/plugin-dialog';
import Notice from './Notice.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { routingNumberOk } from './PartyList.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { addDays, formatDate } from '../lib/dates.js';

/** The next weekday after `iso` (bank holidays are the bank's to skip). */
export function nextBusinessDay(iso) {
  let day = addDays(iso, 1);
  while ([0, 6].includes(new Date(`${day}T12:00:00`).getDay())) day = addDays(day, 1);
  return day;
}

/** Asks where to save an ACH file; `null` when the user cancels. */
export const chooseAchPath = (defaultPath) =>
  save({ defaultPath, filters: [{ name: 'NACHA ACH file', extensions: ['ach', 'txt'] }] });

const BLANK = {
  immediateDestination: '',
  immediateDestinationName: '',
  immediateOrigin: '',
  immediateOriginName: '',
  companyName: '',
  companyId: '',
  entryDescription: 'VENDOR PAY',
};

/** A bank account's ACH originator details, as the bank gave them when it set up ACH origination. */
export function OriginatorDialog({ account, current, onClose, onSaved }) {
  const [form, setForm] = useState({ ...BLANK, ...(current ?? {}), accountId: account.id });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const set = (key) => (e) => setForm((f) => ({ ...f, [key]: e.target.value }));
  const destination = form.immediateDestination.replace(/\D/g, '');
  const destinationBad = destination !== '' && !routingNumberOk(destination);

  const submit = async (e) => {
    e.preventDefault();
    setBusy(true);
    try {
      onSaved(await api.setAchOriginator(form));
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  const field = (label, key, hint, props = {}) => (
    <label className="flex flex-col gap-0.5">
      <span className="font-semibold text-qb-ink-muted">{label}</span>
      <input className="qb-input" value={form[key]} onChange={set(key)} autoComplete="off" spellCheck={false} {...props} />
      {hint && <span className="text-2xs text-qb-ink-faint">{hint}</span>}
    </label>
  );

  return (
    <Dialog title={`ACH details for ${account.name}`} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <p className="text-qb-ink-muted">Your bank gives you these when it sets you up to send ACH files. Type them exactly as the bank wrote them.</p>
        <div className="grid grid-cols-2 gap-2">
          {field('Immediate destination (bank routing number)', 'immediateDestination', destinationBad ? 'This routing number fails its check digit.' : '9 digits', {
            inputMode: 'numeric',
            'aria-invalid': destinationBad,
          })}
          {field('Bank name', 'immediateDestinationName', 'Up to 23 characters', { maxLength: 23 })}
          {field('Immediate origin', 'immediateOrigin', 'Usually 10 characters, such as 1 and your EIN')}
          {field('Company name for the bank', 'immediateOriginName', 'Up to 23 characters', { maxLength: 23 })}
          {field('Company name on statements', 'companyName', 'Up to 16 characters; vendors see it', { maxLength: 16 })}
          {field('Company ID', 'companyId', 'Up to 10 characters, often 1 and your EIN', { maxLength: 10 })}
          {field('Entry description', 'entryDescription', 'Up to 10 characters; vendors see it', { maxLength: 10 })}
        </div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || destinationBad}>
            Save
          </button>
        </div>
      </form>
    </Dialog>
  );
}

/** ACH files already made: what each paid, and saving one again (the same file, nothing paid twice). */
export function AchFilesPanel({ onClose }) {
  const { data, error, reload } = useLedgerQuery(api.listAchFiles);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(null);

  const saveAgain = async (file) => {
    const path = await chooseAchPath(file.fileName);
    if (!path) return;
    setBusy(file.id);
    try {
      await api.saveAchFile(file.id, path);
      setNotice({ kind: 'success', text: `Saved ${file.fileName} again to ${path}. It is the same file: upload it only if the bank did not receive the first one.` });
      reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex h-row-lg items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">ACH files</h2>
        <span className="text-xs text-qb-ink-muted">Saving a file again gives the identical file; it pays nothing more in LocalBooks.</span>
        <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={onClose}>
          Close
        </button>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load ACH files: ${error}`} />}
      <table className="qb-grid">
        <thead>
          <tr>
            <th className="qb-grid-head">File</th>
            <th className="qb-grid-head">From</th>
            <th className="qb-grid-head">Pays on</th>
            <th className="qb-grid-head">Vendors</th>
            <th className="qb-grid-head text-right">Total</th>
            <th className="qb-grid-head">Saved</th>
            <th className="qb-grid-head" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          {data && data.length === 0 && (
            <tr>
              <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No ACH files yet. Choose Pay by ACH file in Pay bills to make one.</td>
            </tr>
          )}
          {(data ?? []).map((f) => (
            <tr key={f.id} className="qb-grid-row">
              <td className="qb-grid-cell font-semibold">{f.fileName}</td>
              <td className="qb-grid-cell">{f.paymentAccountName}</td>
              <td className="qb-grid-cell">{formatDate(f.effectiveDate)}</td>
              <td className="qb-grid-cell" title={f.entries.map((e) => `${e.vendorName} ${e.accountMasked} · trace ${e.traceNumber}`).join('\n')}>
                {f.entries.map((e) => `${e.vendorName} ${formatCents(e.amountCents)}`).join(', ')}
              </td>
              <td className="qb-grid-cell num">{formatCents(f.totalCents)}</td>
              <td className="qb-grid-cell">{f.timesSaved === 0 ? 'Not yet' : `${f.timesSaved}×`}</td>
              <td className="qb-grid-cell text-right">
                {f.voidedPayments > 0 ? (
                  <span className="text-qb-ink-muted" title="A payment in this file was voided, so the file would still pay it.">Has voided payments</span>
                ) : (
                  <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => saveAgain(f)} disabled={busy !== null}>
                    {busy === f.id ? 'Saving…' : 'Save again…'}
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
