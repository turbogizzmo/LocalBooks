import { useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';
import DateBox from './DateBox.jsx';
import Notice from './Notice.jsx';

const STATUS = { OUTSTANDING: 'Out with the accountant', RETURNED: 'Changes imported', CANCELLED: 'Cancelled' };

/** The total of an entry's debits. */
const entryTotal = (e) => e.lines.reduce((sum, l) => sum + l.debitCents, 0);

/**
 * Settings › File & Data Controls › Accountant's Copy (`accountants_copy.rs`, design in
 * docs/plans/accountants-copy.md). In the client's books: make a copy with a dividing date, then
 * import the accountant's changes (or cancel the copy). In the copy: export the changes.
 */
export default function AccountantsCopy({ isAdmin = true }) {
  const { data: state, error, reload } = useLedgerQuery(api.getAccountantsCopyState);
  const [date, setDate] = useState(() => `${todayIso().slice(0, 4)}-01-01`);
  const [preview, setPreview] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);
  const out = state?.copies.find((c) => c.status === 'OUTSTANDING') ?? null;

  const act = async (fn) => {
    setBusy(true);
    setNotice(null);
    try {
      await fn();
      reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const makeCopy = () =>
    act(async () => {
      const destination = await save({ defaultPath: `Accountant's Copy ${date}.localbooks`, filters: [{ name: 'LocalBooks company file', extensions: ['localbooks'] }] });
      if (!destination) return;
      await api.createAccountantsCopy(date, destination);
      setNotice({ kind: 'success', text: `Accountant's Copy saved to ${destination}. Until its changes come back, nothing can be posted on or before ${formatDate(date)}.` });
    });

  const chooseChanges = () =>
    act(async () => {
      const path = await open({ multiple: false, directory: false, filters: [{ name: "Accountant's changes", extensions: ['json'] }] });
      if (typeof path !== 'string') return;
      setPreview({ path, ...(await api.previewAccountantChanges(path)) });
    });

  const importChanges = () =>
    act(async () => {
      const done = await api.importAccountantChanges(preview.path);
      setPreview(null);
      setNotice({ kind: 'success', text: `Imported the accountant's changes: ${done.entriesPosted} journal entr${done.entriesPosted === 1 ? 'y' : 'ies'} posted, ${done.accountsCreated} account${done.accountsCreated === 1 ? '' : 's'} added.` });
    });

  const exportChanges = () =>
    act(async () => {
      const destination = await save({ defaultPath: `Accountant's changes ${state.thisCopyDividingDate}.json`, filters: [{ name: "Accountant's changes", extensions: ['json'] }] });
      if (!destination) return;
      const file = await api.exportAccountantChanges(destination);
      setNotice({
        kind: 'success',
        text: `Saved ${file.entries.length} journal entr${file.entries.length === 1 ? 'y' : 'ies'} and ${file.accounts.length} new account${file.accounts.length === 1 ? '' : 's'} for the client.${file.leftOut.length ? ` Left out (only journal entries and new accounts travel): ${file.leftOut.join('; ')}.` : ''}`,
      });
    });

  return (
    <section className="qb-panel flex flex-col" aria-label="Accountant's Copy">
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Accountant’s Copy</h2>
        <span className="text-xs text-qb-ink-muted">Your accountant works on the period up to a dividing date while you keep working after it</span>
      </div>
      {error && <Notice kind="error" text={error} />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {state?.thisCopyDividingDate ? (
        <div className="flex flex-wrap items-center gap-2 px-2 py-2 text-sm">
          <span>
            This is an Accountant’s Copy, dividing date <span className="font-semibold">{formatDate(state.thisCopyDividingDate)}</span>. Post journal entries and add accounts on or before it,
            then send the client your changes.
          </span>
          <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" disabled={busy} onClick={exportChanges}>Export Changes for Client…</button>
        </div>
      ) : (
        <div className="flex flex-col gap-2 px-2 py-2 text-sm">
          {out ? (
            <div className="flex flex-wrap items-center gap-2">
              <span>
                A copy is out (dividing date <span className="font-semibold">{formatDate(out.dividingDate)}</span>, made {out.createdAt.slice(0, 10)} by {out.createdBy}): nothing can be posted
                on or before that date until its changes come back.
              </span>
              {isAdmin && (
                <span className="ml-auto flex gap-2">
                  <button type="button" className="qb-btn qb-btn-primary h-row" disabled={busy} onClick={chooseChanges}>Import Accountant’s Changes…</button>
                  <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy} onClick={() => act(() => api.cancelAccountantsCopy())}>Cancel the copy</button>
                </span>
              )}
            </div>
          ) : (
            isAdmin && (
              <div className="flex items-end gap-2">
                <label>
                  <span className="qb-label">Dividing date</span>
                  <DateBox className="qb-input w-[140px]" value={date} onChange={(e) => setDate(e.target.value)} />
                </label>
                <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy || !date} onClick={makeCopy}>Create Accountant’s Copy…</button>
              </div>
            )
          )}
          {preview && (
            <div className="border border-qb-border bg-qb-canvas p-2">
              <p className="font-semibold">
                The accountant’s changes: {preview.entries.length} journal entr{preview.entries.length === 1 ? 'y' : 'ies'}, {preview.accounts.length} new account{preview.accounts.length === 1 ? '' : 's'}
                {preview.accountsExisting.length ? ` (${preview.accountsExisting.length} already in your books, used as they are)` : ''}
              </p>
              <ul className="my-1 max-h-48 overflow-auto text-xs scrollbar-thin">
                {preview.accounts.map((a) => (
                  <li key={a.fullName}>New account: {a.number ? `${a.number} · ` : ''}{a.fullName}</li>
                ))}
                {preview.entries.map((e, i) => (
                  <li key={i}>
                    {formatDate(e.date)} · {e.memo || 'Journal entry'}
                    {e.adjusting ? ' (adjusting)' : ''} · {formatCents(entryTotal(e))}
                  </li>
                ))}
                {preview.leftOut.map((l, i) => (
                  <li key={`left-${i}`} className="text-qb-ink-muted">Not brought back: {l}</li>
                ))}
              </ul>
              <div className="flex gap-2">
                <button type="button" className="qb-btn qb-btn-primary h-row" disabled={busy} onClick={importChanges}>Apply the changes</button>
                <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy} onClick={() => setPreview(null)}>Not now</button>
              </div>
            </div>
          )}
          {(state?.copies ?? []).filter((c) => c.status !== 'OUTSTANDING').length > 0 && (
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head">Dividing date</th>
                  <th className="qb-grid-head">Status</th>
                  <th className="qb-grid-head">Made</th>
                  <th className="qb-grid-head">Closed</th>
                  <th className="qb-grid-head text-right">Entries</th>
                </tr>
              </thead>
              <tbody>
                {state.copies.filter((c) => c.status !== 'OUTSTANDING').map((c) => (
                  <tr key={c.id} className="qb-grid-row">
                    <td className="qb-grid-cell">{formatDate(c.dividingDate)}</td>
                    <td className="qb-grid-cell">{STATUS[c.status]}</td>
                    <td className="qb-grid-cell">{c.createdAt.slice(0, 10)} · {c.createdBy}</td>
                    <td className="qb-grid-cell">{c.closedAt ? `${c.closedAt.slice(0, 10)} · ${c.closedBy}` : ''}</td>
                    <td className="qb-grid-cell num">{c.entriesPosted}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </section>
  );
}
