import { Fragment, useEffect, useRef, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import ClassSelect, { showsClassColumn, useFormClass } from './ClassSelect.jsx';
import Notice from './Notice.jsx';
import { MemorizeDialog } from './Recurring.jsx';
import Attachments from './Attachments.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

export const SOURCE_LABEL = {
  JOURNAL: 'Journal Entry', INVOICE: 'Invoice', RECEIVE_PAYMENT: 'Payment', SALES_RECEIPT: 'Sales Receipt',
  CREDIT_MEMO: 'Credit Memo', BILL: 'Bill', BILL_PAYMENT: 'Bill Payment', VENDOR_CREDIT: 'Vendor Credit',
  CHECK: 'Check', EXPENSE: 'Expense', CARD_CREDIT: 'Credit Card Credit', DEPOSIT: 'Deposit', TRANSFER: 'Transfer', OPENING_BALANCE: 'Opening Balance',
};

let nextKey = 1;
const blankRow = () => ({ key: nextKey++, accountId: null, classId: null, debit: '', credit: '', memo: '' });

/** A new journal entry, or (with `editing` = { entry, lines }) a posted manual entry being edited. */
function JournalForm({ accounts, onPosted, onCancel, editing = null }) {
  const [date, setDate] = useState(editing?.entry.entryDate ?? todayIso());
  const [memo, setMemo] = useState(editing?.entry.memo ?? '');
  const [adjusting, setAdjusting] = useState(editing?.entry.isAdjusting ?? false);
  const [rows, setRows] = useState(() =>
    editing
      ? editing.lines.map((l) => ({ ...blankRow(), accountId: l.accountId, classId: l.classId ?? null, debit: l.debitCents ? centsToInput(l.debitCents) : '', credit: l.creditCents ? centsToInput(l.creditCents) : '', memo: l.memo ?? '' }))
      : [blankRow(), blankRow(), blankRow(), blankRow()],
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const dateRef = useRef(null);
  const formClass = useFormClass(accounts);
  const { classes } = formClass;
  const withClasses = showsClassColumn(formClass, rows);

  useEffect(() => dateRef.current?.focus(), []);

  const update = (key, patch) => setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...patch } : r)));
  const parsed = rows.map((r) => {
    const blank = r.accountId === null && r.debit.trim() === '' && r.credit.trim() === '' && r.memo.trim() === '';
    const debit = r.debit.trim() === '' ? 0 : parseMoneyToCents(r.debit);
    const credit = r.credit.trim() === '' ? 0 : parseMoneyToCents(r.credit);
    const valid = !blank && r.accountId !== null && debit !== null && credit !== null && debit >= 0 && credit >= 0 && (debit > 0) !== (credit > 0);
    return { row: r, blank, debit: debit ?? 0, credit: credit ?? 0, valid };
  });
  const used = parsed.filter((p) => !p.blank);
  const totalDebit = used.reduce((s, p) => s + (p.valid ? p.debit : 0), 0);
  const totalCredit = used.reduce((s, p) => s + (p.valid ? p.credit : 0), 0);
  const difference = totalDebit - totalCredit;
  const invalid = used.filter((p) => !p.valid).length;
  const ready = used.length >= 2 && invalid === 0 && difference === 0 && totalDebit > 0;

  const [memorizing, setMemorizing] = useState(null);
  const save = async (mode) => {
    if (invalid > 0) return setError(`${invalid} line(s) need an account and exactly one of debit or credit.`);
    if (used.length < 2) return setError('A journal entry needs at least two lines.');
    if (difference !== 0) return setError(`Debits and credits are out of balance by ${formatCents(Math.abs(difference))}.`);
    const lines = formClass.apply(
      used.map((p) => ({ account_id: p.row.accountId, debit: p.debit, credit: p.credit, memo: p.row.memo.trim() || null, class_id: p.row.classId })),
      'class_id',
    );
    const warning = formClass.missing(lines, 'class_id');
    if (warning) return setError(warning);
    if (mode === 'memorize') return setMemorizing({ id: null, date, description: memo.trim(), lines });
    setBusy(true);
    try {
      if (editing) {
        await api.replaceJournalEntry(editing.entry.id, { id: null, date, description: memo.trim(), lines }, adjusting);
        onPosted(`Journal entry #${editing.entry.entryNumber} replaced: the original is voided and the edited entry is posted.`);
        return onCancel();
      }
      await api.postJournalEntry({ id: null, date, description: memo.trim(), lines }, adjusting);
      onPosted(`${adjusting ? 'Adjusting journal entry' : 'Journal entry'} posted: ${formatCents(totalDebit)} in balanced debits and credits.`);
      setRows([blankRow(), blankRow(), blankRow(), blankRow()]);
      setMemo('');
      setAdjusting(false);
      formClass.reset();
      setError(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">{editing ? `Edit journal entry #${editing.entry.entryNumber}` : 'Journal entry'}</h2>
        <label>
          <span className="qb-label">Journal date</span>
          <input ref={dateRef} type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Memo</span>
          <input className="qb-input w-[320px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Reason for this entry" />
        </label>
        <label className="flex h-row items-center gap-1 self-end text-sm" title="An accountant's period-end adjustment; listed on the Adjusting Journal Entries report">
          <input type="checkbox" checked={adjusting} onChange={(e) => setAdjusting(e.target.checked)} />
          Adjusting entry
        </label>
        {formClass.header}
      </div>
      <table className="qb-grid">
        <colgroup>
          <col className="w-[28px]" />
          <col className="w-[280px]" />
          <col className="w-[120px]" />
          <col className="w-[120px]" />
          <col />
          {withClasses && <col className="w-[160px]" />}
          <col className="w-[28px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head text-right">#</th>
            <th className="qb-grid-head">Account</th>
            <th className="qb-grid-head text-right">Debits</th>
            <th className="qb-grid-head text-right">Credits</th>
            <th className="qb-grid-head">Description</th>
            {withClasses && <th className="qb-grid-head">Class</th>}
            <th className="qb-grid-head" aria-label="Remove" />
          </tr>
        </thead>
        <tbody>
          {parsed.map((p, i) => (
            <tr key={p.row.key} className="qb-grid-row">
              <td className="qb-grid-cell text-right text-qb-ink-faint">{i + 1}</td>
              <td className="qb-grid-cell-edit">
                <AccountSelect className="" accounts={accounts} value={p.row.accountId} onChange={(accountId) => update(p.row.key, { accountId })} placeholder="Select account" aria-label={`Account ${i + 1}`} />
              </td>
              <td className="qb-grid-cell-edit">
                <input className="num" value={p.row.debit} onChange={(e) => update(p.row.key, { debit: e.target.value, credit: e.target.value ? '' : p.row.credit })} inputMode="decimal" aria-invalid={!p.blank && !p.valid} aria-label={`Debit ${i + 1}`} />
              </td>
              <td className="qb-grid-cell-edit">
                <input className="num" value={p.row.credit} onChange={(e) => update(p.row.key, { credit: e.target.value, debit: e.target.value ? '' : p.row.debit })} inputMode="decimal" aria-invalid={!p.blank && !p.valid} aria-label={`Credit ${i + 1}`} />
              </td>
              <td className="qb-grid-cell-edit">
                <input value={p.row.memo} onChange={(e) => update(p.row.key, { memo: e.target.value })} aria-label={`Description ${i + 1}`} />
              </td>
              {withClasses && (
                <td className="qb-grid-cell-edit">
                  <ClassSelect className="" classes={classes} value={p.row.classId} onChange={(classId) => update(p.row.key, { classId })} aria-label={`Class ${i + 1}`} />
                </td>
              )}
              <td className="qb-grid-cell cell-flush text-center">
                <button type="button" onClick={() => setRows((rs) => (rs.length > 2 ? rs.filter((r) => r.key !== p.row.key) : rs))} className="h-row w-full text-qb-ink-faint hover:text-qb-negative" aria-label={`Remove line ${i + 1}`}>
                  ×
                </button>
              </td>
            </tr>
          ))}
          <tr className="bg-qb-row-header font-semibold">
            <td />
            <td className="px-2 py-0.5">
              <button type="button" className="text-sm text-qb-info hover:underline" onClick={() => setRows((rs) => [...rs, blankRow()])}>
                ＋ Add lines
              </button>
            </td>
            <td className="qb-grid-cell num">{formatCents(totalDebit)}</td>
            <td className="qb-grid-cell num">{formatCents(totalCredit)}</td>
            <td className={`qb-grid-cell ${difference === 0 ? 'text-qb-green-active' : 'text-qb-negative'}`} colSpan={withClasses ? 2 : 1}>
              {difference === 0 ? (totalDebit > 0 ? '✓ Balanced' : '') : `Out of balance by ${formatCents(Math.abs(difference))}`}
            </td>
            <td />
          </tr>
        </tbody>
      </table>
      <div className="flex items-center gap-2 px-2 py-1">
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <button type="button" className="qb-btn qb-btn-secondary" onClick={() => save('memorize')} disabled={busy || !ready} title="Save as a recurring transaction">
            Memorize…
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy || !ready}>
            {busy ? 'Posting…' : editing ? 'Save changes' : 'Save & Post'}
          </button>
        </div>
      </div>
      {memorizing && (
        <MemorizeDialog kind="JOURNAL" payload={memorizing} date={date} defaultName={memo.trim()} onClose={() => setMemorizing(null)} onSaved={(text) => { setMemorizing(null); onPosted(text); }} />
      )}
    </section>
  );
}

export function EntryLines({ entryId }) {
  const [lines, setLines] = useState(null);
  const [error, setError] = useState(null);
  useEffect(() => {
    api.getJournalEntryLines(entryId).then(setLines, (err) => setError(errorText(err)));
  }, [entryId]);
  if (error) return <div className="px-2 py-0.5 text-sm text-qb-negative">{error}</div>;
  if (!lines) return <div className="px-2 py-0.5 text-sm text-qb-ink-muted">Loading…</div>;
  return (
    <>
    <table className="w-full text-sm">
      <tbody>
        {lines.map((l) => (
          <tr key={l.lineNumber}>
            <td className="w-[48px] py-0.5 pl-8 text-qb-ink-faint">{l.lineNumber}</td>
            <td className="py-0.5">{l.accountName}</td>
            <td className="py-0.5 text-qb-ink-muted">{l.memo ?? ''}</td>
            <td className="w-[160px] py-0.5 text-qb-ink-muted">{l.className ?? ''}</td>
            <td className="num w-[120px] py-0.5">{l.debitCents ? formatCents(l.debitCents) : ''}</td>
            <td className="num w-[120px] py-0.5 pr-[42px]">{l.creditCents ? formatCents(l.creditCents) : ''}</td>
          </tr>
        ))}
      </tbody>
    </table>
    <Attachments entryId={entryId} />
    </>
  );
}

export default function JournalEntries({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listJournalEntries);
  const [composing, setComposing] = useState(intent?.action === 'journal-entry');
  const [expanded, setExpanded] = useState(intent?.action === 'open-entry' ? intent.data.entryId : null);
  const [filter, setFilter] = useState('ALL');
  const [notice, setNotice] = useState(null);
  const [editing, setEditing] = useState(null); // { entry, lines }
  const startEdit = (entry) =>
    api.getJournalEntryLines(entry.id).then(
      (lines) => {
        setEditing({ entry, lines });
        setComposing(true);
      },
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );

  useEffect(() => {
    if (intent?.action === 'journal-entry') setComposing(true);
    if (intent?.action === 'open-entry') {
      setFilter('ALL');
      setExpanded(intent.data.entryId);
      setTimeout(() => document.getElementById(`entry-${intent.data.entryId}`)?.scrollIntoView({ block: 'center' }), 50);
    }
  }, [intent]);

  const voidEntry = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    setNotice({ kind: 'success', text: 'Voided: a reversing journal entry now offsets it.' });
  };
  const rows = (data ?? []).filter((e) => filter === 'ALL' || (filter === 'MANUAL' ? e.sourceType === 'JOURNAL' : filter === 'VOIDS' ? e.isVoided || e.reversesEntryId !== null : filter === 'ADJUSTING' ? e.isAdjusting : true));

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {composing && (
        <JournalForm
          key={editing ? `edit-${editing.entry.id}` : 'new'}
          editing={editing}
          accounts={accounts}
          onCancel={() => {
            setComposing(false);
            setEditing(null);
          }}
          onPosted={(text) => setNotice({ kind: 'success', text })}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load the journal: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Journal</h2>
          <select className="qb-input h-row w-[170px] text-sm" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Show">
            <option value="ALL">All posted entries</option>
            <option value="MANUAL">Manual journal entries</option>
            <option value="VOIDS">Voids and reversals</option>
            <option value="ADJUSTING">Adjusting entries</option>
          </select>
          <span className="text-xs text-qb-ink-muted">Click a row to see its lines</span>
          {!composing && (
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setComposing(true)}>
              New journal entry
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[92px]" />
              <col className="w-[64px]" />
              <col className="w-[120px]" />
              <col />
              <col className="w-[170px]" />
              <col className="w-[120px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">No.</th>
                <th className="qb-grid-head">Source</th>
                <th className="qb-grid-head">Memo</th>
                <th className="qb-grid-head">Status</th>
                <th className="qb-grid-head text-right">Amount</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No entries.</td>
                </tr>
              )}
              {rows.map((e) => (
                <Fragment key={e.id}>
                  <tr id={`entry-${e.id}`} className={`qb-grid-row cursor-pointer ${expanded === e.id ? 'is-active' : ''} ${e.isVoided ? 'text-qb-ink-faint' : ''}`} onClick={() => setExpanded(expanded === e.id ? null : e.id)}>
                    <td className="qb-grid-cell">{formatDate(e.entryDate)}</td>
                    <td className="qb-grid-cell">{e.entryNumber}</td>
                    <td className="qb-grid-cell">{e.reversesEntryId !== null ? 'Void reversal' : (SOURCE_LABEL[e.sourceType] ?? e.sourceType)}</td>
                    <td className="qb-grid-cell" title={e.memo ?? ''}>{e.memo ?? ''}</td>
                    <td className="qb-grid-cell text-sm">
                      {e.isVoided ? (
                        <span className="font-semibold text-qb-negative">Voided by #{e.reversedByEntryNumber}</span>
                      ) : e.reversesEntryId !== null ? (
                        <span className="text-qb-ink-muted">Reverses #{e.reversesEntryNumber}</span>
                      ) : (
                        <span className="text-qb-ink-muted">Posted</span>
                      )}
                      {e.isAdjusting && <span className="ml-1 rounded-sm bg-qb-canvas px-1 text-2xs font-semibold uppercase text-qb-ink-muted">Adjusting</span>}
                    </td>
                    <td className="qb-grid-cell num">{formatCents(e.totalCents)}</td>
                    <td className="qb-grid-cell cell-tight text-center" onClick={(ev) => ev.stopPropagation()}>
                      {e.reversesEntryId === null && (
                        <RowMenu
                          items={[
                            ...(e.sourceType === 'JOURNAL' ? [{ label: 'Edit…', disabled: e.isVoided, onSelect: () => startEdit(e) }] : []),
                            voidItem({
                              journalEntryId: e.id,
                              voided: e.isVoided,
                              description: `Entry #${e.entryNumber} (${SOURCE_LABEL[e.sourceType] ?? e.sourceType}, ${formatCents(e.totalCents)}).`,
                              onVoid: voidEntry,
                            }),
                          ]}
                        />
                      )}
                    </td>
                  </tr>
                  {expanded === e.id && (
                    <tr className="bg-qb-canvas">
                      <td colSpan={7} className="border-b border-qb-border">
                        <EntryLines entryId={e.id} />
                      </td>
                    </tr>
                  )}
                </Fragment>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
