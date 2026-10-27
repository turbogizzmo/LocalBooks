import { Fragment, useEffect, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { EntryLines, SOURCE_LABEL } from './JournalEntries.jsx';
import { askConfirm } from '../lib/dialogs.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { addDays, formatDate, periodRange, todayIso } from '../lib/dates.js';

const TOOLS = [
  { id: 'credits', label: 'Unapplied credits' },
  { id: 'undeposited', label: 'Undeposited Funds' },
  { id: 'write-off', label: 'Write off invoices' },
  { id: 'lists', label: 'List changes' },
  { id: 'inventory', label: 'Inventory vs. Balance Sheet' },
];

const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;

/** Runs a fix after the user confirms it, and reports the outcome. */
function useFix(setNotice) {
  const [busy, setBusy] = useState(false);
  const run = async (question, confirmLabel, work, done) => {
    if (!(await askConfirm(question, { confirmLabel }))) return;
    setBusy(true);
    try {
      const result = await work();
      setNotice({ kind: 'success', text: done(result) });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };
  return [busy, run];
}

function Panel({ title, note, children, actions }) {
  return (
    <section className="qb-panel">
      <div className="flex flex-wrap items-center gap-2 border-b border-qb-border px-3 py-2">
        <h3 className="text-sm font-semibold">{title}</h3>
        {note && <span className="text-xs text-qb-ink-muted">{note}</span>}
        {actions && <div className="ml-auto flex items-center gap-2">{actions}</div>}
      </div>
      {children}
    </section>
  );
}

const Empty = ({ text }) => <div className="qb-panel px-3 py-2 text-sm text-qb-ink-muted">{text}</div>;

// ------------------------------------------------------------------------------------------------
// 1. Unapplied credits
// ------------------------------------------------------------------------------------------------

function UnappliedCredits({ setNotice }) {
  const { data, error } = useLedgerQuery(() => api.listUnappliedCredits());
  const [date, setDate] = useState(todayIso);
  // "kind:creditId:documentId" → amount text; filled from the suggestions.
  const [amounts, setAmounts] = useState({});
  const [busy, run] = useFix(setNotice);
  const key = (a) => `${a.kind}:${a.creditId}:${a.documentId}`;

  useEffect(() => {
    if (!data) return;
    setAmounts(Object.fromEntries(data.groups.flatMap((g) => g.suggested.map((a) => [key(a), centsToInput(a.amountCents)]))));
  }, [data]);

  if (error) return <Notice kind="error" text={error} />;
  if (!data) return null;
  const applications = Object.entries(amounts)
    .map(([k, text]) => {
      const [kind, creditId, documentId] = k.split(':');
      return { kind, creditId: Number(creditId), documentId: Number(documentId), amountCents: parseMoneyToCents(text) };
    })
    .filter((a) => a.amountCents > 0);
  const invalid = Object.values(amounts).some((t) => t.trim() !== '' && !(parseMoneyToCents(t) >= 0));
  const total = applications.reduce((s, a) => s + a.amountCents, 0);
  const apply = () =>
    run(
      `Apply ${plural(applications.length, 'credit')} totalling ${formatCents(total)} on ${formatDate(date)}? Nothing is posted; each invoice or bill shows the credit as paid.`,
      'Apply credits',
      () => api.applyCleanupCredits({ appliedDate: date, applications }),
      (r) => `Applied ${plural(r.count, 'credit')} totalling ${formatCents(r.totalCents)}.`,
    );

  return (
    <div className="flex flex-col gap-2">
      {data.groups.length === 0 && <Empty text="Every credit memo and vendor credit is fully used." />}
      {data.groups.map((g) => (
        <Panel key={`${g.kind}${g.partyId}`} title={g.partyName} note={g.kind === 'CUSTOMER' ? 'Customer credits and open invoices' : 'Vendor credits and open bills'}>
          {g.documents.length === 0 ? (
            <div className="px-3 py-2 text-sm text-qb-ink-muted">
              {plural(g.credits.length, 'credit')} with {formatCents(g.credits.reduce((s, c) => s + c.remainingCents, 0))} left, but nothing open to apply it to. Refund it from{' '}
              {g.kind === 'CUSTOMER' ? 'Sales › Credit Memos' : 'Expenses › Vendor Credits'} instead.
            </div>
          ) : (
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head">Apply</th>
                  <th className="qb-grid-head">To</th>
                  <th className="qb-grid-head">Due</th>
                  <th className="qb-grid-head text-right">Open</th>
                  <th className="qb-grid-head w-[130px] text-right">Amount</th>
                </tr>
              </thead>
              <tbody>
                {g.credits.map((c) => (
                  <Fragment key={c.id}>
                    <tr>
                      <td className="qb-grid-cell font-semibold" colSpan={5}>
                        {c.label} of {formatDate(c.date)} · {formatCents(c.remainingCents)} left of {formatCents(c.totalCents)}
                        {c.memo && <span className="ml-1 font-normal text-qb-ink-muted">{c.memo}</span>}
                      </td>
                    </tr>
                    {g.documents.map((doc) => {
                      const k = key({ kind: g.kind, creditId: c.id, documentId: doc.id });
                      return (
                        <tr key={doc.id}>
                          <td className="qb-grid-cell" />
                          <td className="qb-grid-cell">
                            {doc.label}, {formatDate(doc.date)}
                          </td>
                          <td className="qb-grid-cell">{formatDate(doc.dueDate)}</td>
                          <td className="qb-grid-cell num">{formatCents(doc.openCents)}</td>
                          <td className="qb-grid-cell">
                            <input
                              className="qb-input num w-full text-right"
                              inputMode="decimal"
                              placeholder="0.00"
                              aria-label={`Apply ${c.label} to ${doc.label}`}
                              value={amounts[k] ?? ''}
                              onChange={(e) => setAmounts((a) => ({ ...a, [k]: e.target.value }))}
                            />
                          </td>
                        </tr>
                      );
                    })}
                  </Fragment>
                ))}
              </tbody>
            </table>
          )}
        </Panel>
      ))}
      {data.groups.length > 0 && (
        <section className="qb-panel flex flex-wrap items-center gap-3 px-3 py-2">
          <label className="flex items-center gap-2 text-xs">
            <span className="font-semibold text-qb-ink-muted">Apply on</span>
            <input type="date" className="qb-input" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          <span className="text-sm">
            {plural(applications.length, 'application')} · total <strong className="num">{formatCents(total)}</strong>
          </span>
          {invalid && <span className="text-xs text-qb-negative">Check the amounts.</span>}
          <button type="button" className="qb-btn qb-btn-primary ml-auto h-row-lg px-3" disabled={busy || invalid || applications.length === 0 || !date} onClick={apply}>
            Apply credits…
          </button>
        </section>
      )}
      {data.unlinked.length > 0 && (
        <Panel title="Unapplied payments from journal entries" note="Review only">
          <p className="px-3 pt-2 text-xs text-qb-ink-muted">
            These journal entries credit Accounts Receivable (or debit Accounts Payable) without an invoice or bill, usually money an import brought in as unapplied. A journal entry names no
            customer and can’t pay an invoice, so the A/R and A/P aging leave it out. To fix one, void it and record the payment against its invoice or bill.
          </p>
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head">Entry</th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Side</th>
                <th className="qb-grid-head">Memo</th>
                <th className="qb-grid-head text-right">Amount</th>
              </tr>
            </thead>
            <tbody>
              {data.unlinked.map((e) => (
                <tr key={`${e.journalEntryId}${e.side}`}>
                  <td className="qb-grid-cell">#{e.entryNumber}</td>
                  <td className="qb-grid-cell">{formatDate(e.entryDate)}</td>
                  <td className="qb-grid-cell">{e.side === 'AR' ? 'Receivable' : 'Payable'}</td>
                  <td className="qb-grid-cell">{e.memo}</td>
                  <td className="qb-grid-cell num">{formatCents(e.amountCents)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
      )}
    </div>
  );
}

// ------------------------------------------------------------------------------------------------
// 2. Undeposited Funds
// ------------------------------------------------------------------------------------------------

function UndepositedFunds({ setNotice }) {
  const { data, error } = useLedgerQuery(() => api.listUndepositedReview());
  // "KIND:id" → deposit line id
  const [chosen, setChosen] = useState({});
  const [busy, run] = useFix(setNotice);
  useEffect(() => setChosen({}), [data]);
  if (error) return <Notice kind="error" text={error} />;
  if (!data) return null;

  const lineOf = (id) => data.customers.flatMap((c) => c.candidates).find((l) => l.depositLineId === id);
  const picks = Object.entries(chosen)
    .filter(([, line]) => line)
    .map(([k, depositLineId]) => {
      const [kind, id] = k.split(':');
      const payment = data.customers.flatMap((c) => c.payments).find((p) => p.kind === kind && p.id === Number(id));
      const line = lineOf(depositLineId);
      return { kind, id: Number(id), depositLineId, entryDate: line.depositDate, payment, line };
    });
  const total = picks.reduce((s, p) => s + p.payment.amountCents, 0);
  const match = () => {
    const accounts = [...new Set(picks.map((p) => p.line.lineAccountName))].join(', ');
    run(
      `Match ${plural(picks.length, 'payment')} totalling ${formatCents(total)} to deposits that already banked them? A correcting entry on each deposit’s date moves the money out of Undeposited Funds and off ${accounts}. The deposits themselves don’t change.`,
      'Match payments',
      () => api.matchUndepositedFunds(picks.map(({ kind, id, depositLineId, entryDate }) => ({ kind, id, depositLineId, entryDate }))),
      (r) => `Matched ${plural(r.count, 'payment')} totalling ${formatCents(r.totalCents)}; they are out of Undeposited Funds.`,
    );
  };
  const undo = (m) =>
    run(
      `Undo the match of ${formatCents(m.amountCents)} from ${m.customerName ?? 'this customer'}? ${m.entryNumber ? `Entry #${m.entryNumber} is voided and the` : 'The'} payment goes back into Undeposited Funds.`,
      'Undo match',
      () => api.unmatchUndepositedFunds(m.id),
      () => 'Match undone; the payment is back in Undeposited Funds.',
    );

  return (
    <div className="flex flex-col gap-2">
      {data.customers.length === 0 && <Empty text="Nothing is waiting in Undeposited Funds." />}
      {data.customers.map((c) => (
        <Panel key={c.customerName ?? ''} title={c.customerName ?? 'No customer'} note={`${formatCents(c.totalCents)} in Undeposited Funds`}>
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head">Payment</th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Ref.</th>
                <th className="qb-grid-head text-right">Amount</th>
                <th className="qb-grid-head w-[45%]">Already banked by</th>
              </tr>
            </thead>
            <tbody>
              {c.payments.map((p) => {
                const k = `${p.kind}:${p.id}`;
                const fits = c.candidates.filter((l) => l.availableCents >= p.amountCents);
                return (
                  <tr key={k}>
                    <td className="qb-grid-cell">{p.kind === 'PAYMENT' ? 'Invoice payment' : 'Sales receipt'}</td>
                    <td className="qb-grid-cell">{formatDate(p.date)}</td>
                    <td className="qb-grid-cell">{p.reference}</td>
                    <td className="qb-grid-cell num">{formatCents(p.amountCents)}</td>
                    <td className="qb-grid-cell">
                      {fits.length === 0 ? (
                        <span className="text-xs text-qb-ink-muted">No deposit line could hold it; deposit it from Banking › Deposits.</span>
                      ) : (
                        <select
                          className="qb-input w-full"
                          aria-label={`Deposit for the ${formatCents(p.amountCents)} payment of ${formatDate(p.date)}`}
                          value={chosen[k] ?? ''}
                          onChange={(e) => setChosen((s) => ({ ...s, [k]: e.target.value ? Number(e.target.value) : null }))}
                        >
                          <option value="">Leave in Undeposited Funds</option>
                          {fits.map((l) => (
                            <option key={l.depositLineId} value={l.depositLineId}>
                              {l.availableCents === p.amountCents ? '= ' : ''}
                              {formatDate(l.depositDate)} · {l.bankAccountName} · {formatCents(l.availableCents)} to {l.lineAccountName}
                              {l.depositMemo || l.lineMemo ? ` · ${[l.depositMemo, l.lineMemo].filter(Boolean).join(' / ')}` : ''}
                              {l.namesCustomer ? '' : ' (no customer named)'}
                            </option>
                          ))}
                        </select>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Panel>
      ))}
      {data.customers.length > 0 && (
        <section className="qb-panel flex flex-wrap items-center gap-3 px-3 py-2">
          <span className="text-sm">
            {plural(picks.length, 'payment')} to match · total <strong className="num">{formatCents(total)}</strong>
          </span>
          <button type="button" className="qb-btn qb-btn-primary ml-auto h-row-lg px-3" disabled={busy || picks.length === 0} onClick={match}>
            Match to deposits…
          </button>
        </section>
      )}
      {data.matches.length > 0 && (
        <Panel title="Matched earlier" note="Payments that now count as deposited">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head">Customer</th>
                <th className="qb-grid-head">Payment date</th>
                <th className="qb-grid-head">Deposit</th>
                <th className="qb-grid-head">Correcting entry</th>
                <th className="qb-grid-head text-right">Amount</th>
                <th className="qb-grid-head" />
              </tr>
            </thead>
            <tbody>
              {data.matches.map((m) => (
                <tr key={m.id}>
                  <td className="qb-grid-cell">{m.customerName ?? '—'}</td>
                  <td className="qb-grid-cell">{formatDate(m.itemDate)}</td>
                  <td className="qb-grid-cell">
                    {formatDate(m.depositDate)}, line to {m.lineAccountName}
                  </td>
                  <td className="qb-grid-cell">{m.entryNumber ? `#${m.entryNumber}` : 'None needed'}</td>
                  <td className="qb-grid-cell num">{formatCents(m.amountCents)}</td>
                  <td className="qb-grid-cell text-right">
                    <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy} onClick={() => undo(m)}>
                      Undo
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
      )}
    </div>
  );
}

// ------------------------------------------------------------------------------------------------
// 3. Write off invoices
// ------------------------------------------------------------------------------------------------

const isWriteOffAccount = (a) => (a.classification === 'INCOME' || a.classification === 'EXPENSE') && !a.isSystem;

function WriteOff({ accounts, setNotice }) {
  const [asOf, setAsOf] = useState(todayIso);
  const [minDays, setMinDays] = useState('180');
  const [maxOpen, setMaxOpen] = useState('');
  const [accountId, setAccountId] = useState(() => accounts.find((a) => /bad debt/i.test(a.name) && isWriteOffAccount(a))?.id ?? null);
  const [memo, setMemo] = useState('');
  const [picked, setPicked] = useState(() => new Set());
  const [busy, run] = useFix(setNotice);
  const filter = { asOf, minDaysOverdue: Math.max(0, Number(minDays) || 0), maxOpenCents: maxOpen.trim() ? parseMoneyToCents(maxOpen) : null };
  const { data, error, reload } = useLedgerQuery(() => api.previewWriteOffs(filter));
  useEffect(() => {
    if (asOf) reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asOf, minDays, maxOpen]);
  useEffect(() => setPicked(new Set()), [data]);

  const rows = data ?? [];
  const chosen = rows.filter((r) => picked.has(r.invoiceId));
  const total = chosen.reduce((s, r) => s + r.openCents, 0);
  const account = accounts.find((a) => a.id === accountId);
  const toggle = (id) =>
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const writeOff = () =>
    run(
      `Write off ${plural(chosen.length, 'invoice')} totalling ${formatCents(total)} to ${account.fullName} on ${formatDate(asOf)}? Each gets a credit memo for its open balance, applied to it.`,
      'Write off',
      () => api.writeOffInvoices({ invoiceIds: chosen.map((r) => r.invoiceId), accountId, writeOffDate: asOf, memo: memo.trim() || null }),
      (r) => `Wrote off ${plural(r.count, 'invoice')} totalling ${formatCents(r.totalCents)} with credit memos.`,
    );

  return (
    <div className="flex flex-col gap-2">
      <section className="qb-panel flex flex-wrap items-end gap-3 px-3 py-2">
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Write-off date</span>
          <input type="date" className="qb-input" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">At least days past due</span>
          <input className="qb-input num w-[90px]" inputMode="numeric" value={minDays} onChange={(e) => setMinDays(e.target.value)} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Open balance up to</span>
          <input className="qb-input num w-[110px]" inputMode="decimal" placeholder="Any" value={maxOpen} onChange={(e) => setMaxOpen(e.target.value)} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Write off to</span>
          <AccountSelect accounts={accounts} filter={isWriteOffAccount} value={accountId} onChange={setAccountId} placeholder="Choose a bad debt or discount account" className="qb-input w-[260px]" />
        </label>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Memo</span>
          <input className="qb-input w-[200px]" value={memo} onChange={(e) => setMemo(e.target.value)} />
        </label>
      </section>
      {error && <Notice kind="error" text={error} />}
      {data && rows.length === 0 && <Empty text="No open invoice matches these filters." />}
      {rows.length > 0 && (
        <section className="qb-panel">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head w-[32px]">
                  <input type="checkbox" className="accent-qb-green" aria-label="Select all" checked={chosen.length === rows.length} onChange={(e) => setPicked(new Set(e.target.checked ? rows.map((r) => r.invoiceId) : []))} />
                </th>
                <th className="qb-grid-head">Invoice</th>
                <th className="qb-grid-head">Customer</th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Due</th>
                <th className="qb-grid-head text-right">Days past due</th>
                <th className="qb-grid-head text-right">Total</th>
                <th className="qb-grid-head text-right">Open balance</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.invoiceId}>
                  <td className="qb-grid-cell">
                    <input type="checkbox" className="accent-qb-green" aria-label={`Write off invoice #${r.invoiceNumber}`} checked={picked.has(r.invoiceId)} onChange={() => toggle(r.invoiceId)} />
                  </td>
                  <td className="qb-grid-cell">#{r.invoiceNumber}</td>
                  <td className="qb-grid-cell">{r.customerName}</td>
                  <td className="qb-grid-cell">{formatDate(r.issueDate)}</td>
                  <td className="qb-grid-cell">{formatDate(r.dueDate)}</td>
                  <td className="qb-grid-cell num">{r.daysOverdue}</td>
                  <td className="qb-grid-cell num">{formatCents(r.totalCents)}</td>
                  <td className="qb-grid-cell num">{formatCents(r.openCents)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="flex flex-wrap items-center gap-3 border-t border-qb-border px-3 py-2">
            <span className="text-sm">
              {chosen.length} of {rows.length} selected · total <strong className="num">{formatCents(total)}</strong> · all listed {formatCents(rows.reduce((s, r) => s + r.openCents, 0))}
            </span>
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row-lg px-3" disabled={busy || chosen.length === 0 || !account} onClick={writeOff}>
              Write off…
            </button>
          </div>
        </section>
      )}
    </div>
  );
}

// ------------------------------------------------------------------------------------------------
// 4. List changes
// ------------------------------------------------------------------------------------------------

const LIST_LABEL = { ACCOUNT: 'Account', ITEM: 'Item', CUSTOMER: 'Customer', VENDOR: 'Vendor' };
const CHANGE_LABEL = { ADDED: 'Added', RENAMED: 'Renamed', MADE_INACTIVE: 'Made inactive', MADE_ACTIVE: 'Made active', EDITED: 'Edited' };

function ListChanges() {
  const [[start, end], setRange] = useState(() => periodRange('thisYearToDate'));
  const [list, setList] = useState('');
  const { data, error, reload } = useLedgerQuery(() => api.listListChanges(start, end));
  useEffect(() => {
    if (start && end && start <= end) reload();
  }, [start, end, reload]);
  const rows = (data ?? []).filter((r) => !list || r.list === list);
  const counts = Object.fromEntries(Object.keys(CHANGE_LABEL).map((k) => [k, rows.filter((r) => r.change === k).length]));

  return (
    <div className="flex flex-col gap-2">
      <section className="qb-panel flex flex-wrap items-end gap-3 px-3 py-2">
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">From</span>
          <input type="date" className="qb-input" value={start} onChange={(e) => setRange([e.target.value, end])} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">To</span>
          <input type="date" className="qb-input" value={end} onChange={(e) => setRange([start, e.target.value])} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">List</span>
          <select className="qb-input" value={list} onChange={(e) => setList(e.target.value)}>
            <option value="">All lists</option>
            {Object.entries(LIST_LABEL).map(([k, v]) => (
              <option key={k} value={k}>
                {v}s
              </option>
            ))}
          </select>
        </label>
        <span className="pb-1 text-xs text-qb-ink-muted">
          {Object.entries(counts)
            .filter(([, n]) => n > 0)
            .map(([k, n]) => `${n} ${CHANGE_LABEL[k].toLowerCase()}`)
            .join(' · ') || 'No changes'}
          . LocalBooks has no list merge, so there are no merges to review.
        </span>
      </section>
      {error && <Notice kind="error" text={error} />}
      {rows.length > 0 && (
        <section className="qb-panel">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head">When</th>
                <th className="qb-grid-head">List</th>
                <th className="qb-grid-head">Name</th>
                <th className="qb-grid-head">Change</th>
                <th className="qb-grid-head">Details</th>
                <th className="qb-grid-head">By</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={i}>
                  <td className="qb-grid-cell whitespace-nowrap">{new Date(r.occurredAt).toLocaleString()}</td>
                  <td className="qb-grid-cell">{LIST_LABEL[r.list]}</td>
                  <td className="qb-grid-cell">{r.name}</td>
                  <td className="qb-grid-cell">{CHANGE_LABEL[r.change]}</td>
                  <td className="qb-grid-cell">{r.details}</td>
                  <td className="qb-grid-cell">{r.actor ?? ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </div>
  );
}

// ------------------------------------------------------------------------------------------------
// 5. Inventory vs. Balance Sheet
// ------------------------------------------------------------------------------------------------

function InventoryCompare() {
  const [asOf, setAsOf] = useState(todayIso);
  const [open, setOpen] = useState(null);
  const { data, error, reload } = useLedgerQuery(() => api.previewInventoryComparison(asOf));
  useEffect(() => {
    if (asOf) reload();
  }, [asOf, reload]);

  return (
    <div className="flex flex-col gap-2">
      <section className="qb-panel flex flex-wrap items-end gap-3 px-3 py-2">
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">As of</span>
          <input type="date" className="qb-input" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
        </label>
        <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setAsOf(addDays(todayIso().slice(0, 4) + '-01-01', -1))}>
          Last year end
        </button>
      </section>
      {error && <Notice kind="error" text={error} />}
      {data && data.accounts.length === 0 && <Empty text="No inventory asset account: no products are tracked as inventory." />}
      {data && data.accounts.length > 0 && (
        <>
          <Panel title="Balance Sheet against Inventory Valuation" note={`as of ${formatDate(data.asOf)}`}>
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head">Account</th>
                  <th className="qb-grid-head text-right">Balance Sheet</th>
                  <th className="qb-grid-head text-right">Inventory Valuation</th>
                  <th className="qb-grid-head text-right">Difference</th>
                </tr>
              </thead>
              <tbody>
                {data.accounts.map((a) => (
                  <tr key={a.accountId}>
                    <td className="qb-grid-cell">{a.accountName}</td>
                    <td className="qb-grid-cell num">{formatCents(a.balanceCents)}</td>
                    <td className="qb-grid-cell num">{formatCents(a.valuationCents)}</td>
                    <td className={`qb-grid-cell num ${a.differenceCents ? 'text-qb-negative' : ''}`}>{formatCents(a.differenceCents)}</td>
                  </tr>
                ))}
                <tr className="font-semibold">
                  <td className="qb-grid-cell">Total</td>
                  <td className="qb-grid-cell num">{formatCents(data.totalBalanceCents)}</td>
                  <td className="qb-grid-cell num">{formatCents(data.totalValuationCents)}</td>
                  <td className="qb-grid-cell num">{formatCents(data.totalDifferenceCents)}</td>
                </tr>
              </tbody>
            </table>
          </Panel>
          {data.differences.length === 0 ? (
            <Empty text="They agree: every amount on the inventory accounts moved the same value of stock." />
          ) : (
            <Panel title="Transactions that cause the difference" note="Posted to an inventory account without moving the same value of stock. Click one to see its lines.">
              <table className="qb-grid">
                <thead>
                  <tr>
                    <th className="qb-grid-head">Date</th>
                    <th className="qb-grid-head">Entry</th>
                    <th className="qb-grid-head">Type</th>
                    <th className="qb-grid-head">Name / memo</th>
                    <th className="qb-grid-head">Account</th>
                    <th className="qb-grid-head text-right">On the account</th>
                    <th className="qb-grid-head text-right">Stock moved</th>
                    <th className="qb-grid-head text-right">Difference</th>
                  </tr>
                </thead>
                <tbody>
                  {data.differences.map((d) => {
                    const k = `${d.journalEntryId}:${d.accountId}`;
                    return (
                      <Fragment key={k}>
                        <tr className="cursor-pointer hover:bg-qb-row-hover" onClick={() => setOpen(open === k ? null : k)}>
                          <td className="qb-grid-cell">{formatDate(d.entryDate)}</td>
                          <td className="qb-grid-cell">
                            {open === k ? '▾' : '▸'} #{d.entryNumber}
                          </td>
                          <td className="qb-grid-cell">{SOURCE_LABEL[d.sourceType] ?? d.sourceType}</td>
                          <td className="qb-grid-cell">{[d.partyName, d.memo].filter(Boolean).join(' · ')}</td>
                          <td className="qb-grid-cell">{d.accountName}</td>
                          <td className="qb-grid-cell num">{formatCents(d.ledgerCents)}</td>
                          <td className="qb-grid-cell num">{formatCents(d.stockCents)}</td>
                          <td className="qb-grid-cell num">{formatCents(d.differenceCents)}</td>
                        </tr>
                        {open === k && (
                          <tr>
                            <td className="qb-grid-cell" colSpan={8}>
                              <EntryLines entryId={d.journalEntryId} />
                            </td>
                          </tr>
                        )}
                      </Fragment>
                    );
                  })}
                </tbody>
              </table>
              <p className="px-3 py-2 text-xs text-qb-ink-muted">
                To fix one, void it and record the stock with Adjust stock… in Sales › Products & Services, or move the amount to the right account with a journal entry.
              </p>
            </Panel>
          )}
        </>
      )}
    </div>
  );
}

/** Accounting › Clean Up Books: QuickBooks Accountant's Client Data Review tools in one place. */
export default function CleanUpBooks({ accounts }) {
  const [tool, setTool] = useState('credits');
  const [notice, setNotice] = useState(null);
  return (
    <div className="flex h-full flex-col gap-2 overflow-auto p-2 scrollbar-thin">
      <section className="qb-panel">
        <div className="flex flex-wrap items-center gap-2 border-b border-qb-border px-3 py-2">
          <h2 className="text-md font-semibold">Clean up books</h2>
          <span className="text-xs text-qb-ink-muted">Find and fix common problems before closing a year. Every fix asks first and is recorded in the audit log.</span>
        </div>
        <div className="flex flex-wrap gap-1 px-3 py-2" role="tablist">
          {TOOLS.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tool === t.id}
              className={`qb-btn h-row px-3 ${tool === t.id ? 'qb-btn-primary' : 'qb-btn-secondary'}`}
              onClick={() => {
                setTool(t.id);
                setNotice(null);
              }}
            >
              {t.label}
            </button>
          ))}
        </div>
      </section>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {tool === 'credits' && <UnappliedCredits setNotice={setNotice} />}
      {tool === 'undeposited' && <UndepositedFunds setNotice={setNotice} />}
      {tool === 'write-off' && <WriteOff accounts={accounts} setNotice={setNotice} />}
      {tool === 'lists' && <ListChanges />}
      {tool === 'inventory' && <InventoryCompare />}
    </div>
  );
}
