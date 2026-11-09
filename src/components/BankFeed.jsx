import { useEffect, useMemo, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import ClassSelect, { useClassTracking } from './ClassSelect.jsx';
import Notice from './Notice.jsx';
import BankColumnMapping from './BankColumnMapping.jsx';
import BankConnections, { syncSummaryText } from './BankConnections.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import BankRules, { RuleFields, suggestedPattern } from './BankRules.jsx';

/** "Check #1042 · Landlord, 03/01/2026": an entered transaction a bank line matches. */
export const matchLabel = (m) =>
  `${SOURCE_LABEL[m.sourceType] ?? m.sourceType}${m.num ? ` #${m.num}` : ''}${m.name ? ` · ${m.name}` : ''}, ${formatDate(m.date)}`;

const ORIGIN_TEXT = {
  LEARNED: 'Suggested by a rule learned from your past approvals',
  BUILT_IN: 'Suggested by vendor match',
  MODEL: 'Suggested from similar past transactions',
  DEFAULT: 'No match found: uncategorized',
};

const LEVEL_TEXT = { HIGH: 'High', MEDIUM: 'Medium', LOW: 'Low' };
const LEVEL_CLASS = {
  HIGH: 'border-qb-green text-qb-green-active',
  MEDIUM: 'border-qb-border-strong text-qb-ink',
  LOW: 'border-qb-border text-qb-ink-muted',
};

/** The confidence of a suggestion made from similar past transactions, as a small badge. */
function Confidence({ suggestion }) {
  if (suggestion?.origin !== 'MODEL') return null;
  const level = suggestion.confidenceLevel;
  return (
    <span
      className={`ml-1 inline-block flex-none whitespace-nowrap rounded-xs border px-1 text-2xs font-semibold leading-tight ${LEVEL_CLASS[level]}`}
      title={`${suggestion.confidence}% confident, from similar past transactions`}
    >
      {LEVEL_TEXT[level]} {suggestion.confidence}%
    </span>
  );
}

const isFeedAccount = (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD';

export default function BankFeed({ accounts }) {
  const feedAccounts = useMemo(() => accounts.filter((a) => a.isActive && isFeedAccount(a)), [accounts]);
  const [bankAccountId, setBankAccountId] = useState(null);
  const { data, error } = useLedgerQuery(api.listBankTransactions);
  const { data: vendors } = useLedgerQuery(api.listVendors);
  const [selectedId, setSelectedId] = useState(null);
  const [overrides, setOverrides] = useState({});
  const [payees, setPayees] = useState({}); // bank line id → payee typed by the user
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  const [flashId, setFlashId] = useState(null);
  // A file waiting for the user to confirm its columns: { path, inspection }.
  const [mappingFile, setMappingFile] = useState(null);
  const listRef = useRef(null);
  const [showConnections, setShowConnections] = useState(false);
  // Matching: other entered transactions the selected line could be, the Show matched switch, and
  // the lines ticked for Add several at once.
  const [candidates, setCandidates] = useState(null); // { id, list } for the selected line
  const [showMatched, setShowMatched] = useState(false);
  const matched = useLedgerQuery(api.listMatchedBankTransactions);
  const [ticked, setTicked] = useState(() => new Set());
  // Renaming rules: the rules list, and the rule offered after the user changes a payee.
  const [showRules, setShowRules] = useState(false);
  const [offer, setOffer] = useState(null); // { matchKind, pattern, payeeName, description }
  // SimpleFIN connections with at least one linked account, for the Sync button.
  const [syncable, setSyncable] = useState([]);

  const loadConnections = () =>
    api.listBankConnections().then(
      (list) => setSyncable(list.filter((c) => c.hasAccess && c.accounts.some((a) => a.bankAccountId !== null))),
      () => setSyncable([]),
    );
  useEffect(() => {
    loadConnections();
  }, []);

  const syncAll = async () => {
    setBusy(true);
    setNotice(null);
    const texts = [];
    let failed = false;
    for (const c of syncable) {
      try {
        texts.push(syncSummaryText(await api.syncBankConnection(c.id)));
      } catch (err) {
        failed = true;
        texts.push(`${c.serverHost}: ${errorText(err)}`);
      }
    }
    setNotice({ kind: failed ? 'error' : 'success', text: texts.join(' ') });
    setBusy(false);
    loadConnections();
  };

  useEffect(() => {
    if (bankAccountId === null && feedAccounts.length > 0) setBankAccountId(feedAccounts[0].id);
  }, [bankAccountId, feedAccounts]);

  const pending = useMemo(
    () => (data ?? []).filter((t) => t.bankAccountId === bankAccountId),
    [data, bankAccountId],
  );
  const pendingByAccount = useMemo(() => {
    const counts = new Map();
    for (const t of data ?? []) counts.set(t.bankAccountId, (counts.get(t.bankAccountId) ?? 0) + 1);
    return counts;
  }, [data]);

  // Keep a valid selection as the queue changes.
  useEffect(() => {
    if (pending.length === 0) setSelectedId(null);
    else if (!pending.some((t) => t.id === selectedId)) setSelectedId(pending[0].id);
  }, [pending, selectedId]);

  const selectedIndex = pending.findIndex((t) => t.id === selectedId);
  const selected = selectedIndex >= 0 ? pending[selectedIndex] : null;
  const categoryId = selected ? (overrides[selected.id] ?? selected.suggestion?.accountId ?? null) : null;
  const tracking = useClassTracking();
  const [classes, setClasses] = useState({}); // bank line id → chosen class
  const classId = selected ? (classes[selected.id] ?? null) : null;
  // Only money out has a payee: it becomes the vendor on the expense.
  const payee = selected && selected.amountCents < 0 ? (payees[selected.id] ?? selected.payee ?? '') : '';

  const reportImport = (r, path) => {
    const extras = [
      r.matched ? `${r.matched} matched to transactions already entered` : null,
      r.duplicates ? `${r.duplicates} already imported` : null,
      r.skippedZero ? `${r.skippedZero} zero-amount skipped` : null,
    ].filter(Boolean);
    setNotice({
      kind: 'success',
      text: `Imported ${r.imported} transaction${r.imported === 1 ? '' : 's'} from ${path.split(/[\\/]/).pop()} into ${accountLabel(accounts, bankAccountId)}${extras.length ? ` (${extras.join(', ')})` : ''}.`,
    });
  };

  // OFX/QFX and recognized CSV layouts import straight away; otherwise the user maps the columns.
  // A PDF statement always shows its columns and balance check first: its table was read from the
  // page layout, so the user confirms it.
  const importStatement = async () => {
    if (bankAccountId === null) return;
    try {
      const path = await open({
        multiple: false,
        directory: false,
        filters: [{ name: 'Bank statement (CSV, OFX, QFX, PDF)', extensions: ['csv', 'CSV', 'ofx', 'OFX', 'qfx', 'QFX', 'qbo', 'QBO', 'pdf', 'PDF'] }],
      });
      if (!path) return;
      setBusy(true);
      setNotice(null);
      const inspection = await api.inspectBankFile(path, null, bankAccountId);
      const confident = inspection.format === 'OFX' || (inspection.format === 'CSV' && inspection.mapping && inspection.dateOrderCertain);
      if (inspection.format === 'OFX' && inspection.problem) throw new Error(inspection.problem);
      if (!confident) {
        setMappingFile({ path, inspection });
        return;
      }
      reportImport(await api.importBankFile(path, bankAccountId), path);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const advanceFrom = (index) => {
    const next = pending[index + 1] ?? pending[index - 1] ?? null;
    setSelectedId(next ? next.id : null);
  };

  const approve = async () => {
    if (!selected || categoryId === null) return;
    setBusy(true);
    try {
      await api.approveBankTransaction(selected.id, categoryId, classId, payee.trim() || null);
      setFlashId(selected.id);
      // A payee the user typed in place of the suggested one: offer a renaming rule for next time.
      // Rules ignore capitals and spacing, so a change of only those offers nothing.
      const squeeze = (text) => text.trim().replace(/\s+/g, ' ').toLowerCase();
      const typed = (payees[selected.id] ?? '').trim().replace(/\s+/g, ' ');
      if (selected.amountCents < 0 && typed && squeeze(typed) !== squeeze(selected.payee ?? '')) {
        setOffer({ id: 0, matchKind: 'BEGINS', pattern: suggestedPattern(selected.description), payeeName: typed, description: selected.description });
      }
      setNotice({
        kind: 'success',
        text: `Approved ${formatCents(Math.abs(selected.amountCents))} "${selected.description}" → ${accountLabel(accounts, categoryId)}.`,
      });
      advanceFrom(selectedIndex);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const exclude = async () => {
    if (!selected) return;
    setBusy(true);
    try {
      await api.excludeBankTransaction(selected.id);
      setNotice({ kind: 'info', text: `Excluded "${selected.description}" from the review queue.` });
      advanceFrom(selectedIndex);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  // Match / Unmatch and Add several at once.
  const run = async (work, text) => {
    setBusy(true);
    try {
      await work();
      setNotice({ kind: 'success', text });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };
  const matchTo = (line, m) => {
    // Where the line was before the list refreshes without it.
    const index = pending.findIndex((t) => t.id === line.id);
    return run(async () => {
      await api.matchBankTransaction(line.id, m.journalEntryId);
      setCandidates(null);
      if (index >= 0) advanceFrom(index);
      else setSelectedId(null);
    }, `Matched "${line.description}" to ${matchLabel(m)}: nothing new posts.`);
  };
  const findMatches = async () => {
    if (!selected) return;
    try {
      setCandidates({ id: selected.id, list: await api.listBankMatchCandidates(selected.id) });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const unmatch = (line) => run(() => api.unmatchBankTransaction(line.id), `"${line.description}" is back for review.`);
  const tickedLines = pending.filter((t) => ticked.has(t.id));
  const lineCategory = (t) => overrides[t.id] ?? t.suggestion?.accountId ?? null;
  const addTicked = () =>
    run(async () => {
      if (tickedLines.some((t) => lineCategory(t) === null)) throw new Error('Choose a category for every ticked line first.');
      await api.approveBankTransactions(
        tickedLines.map((t) => ({
          transactionId: t.id,
          categoryAccountId: lineCategory(t),
          classId: classes[t.id] ?? null,
          payeeName: t.amountCents < 0 ? (payees[t.id] ?? t.payee ?? '').trim() || null : null,
        })),
      );
      setTicked(new Set());
    }, `Added ${tickedLines.length} transaction${tickedLines.length === 1 ? '' : 's'}.`);
  const toggleTick = (id) =>
    setTicked((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const matchedHere = (matched.data ?? []).filter((m) => m.bankAccountId === bankAccountId);

  useEffect(() => {
    if (flashId === null) return undefined;
    const t = setTimeout(() => setFlashId(null), 900);
    return () => clearTimeout(t);
  }, [flashId]);

  const onListKey = (e) => {
    if (pending.length === 0) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const delta = e.key === 'ArrowDown' ? 1 : -1;
      const next = Math.min(pending.length - 1, Math.max(0, selectedIndex + delta));
      setSelectedId(pending[next].id);
      listRef.current?.querySelector(`[data-id="${pending[next].id}"]`)?.scrollIntoView({ block: 'nearest' });
    } else if (e.key === 'Enter' && !busy) {
      e.preventDefault();
      approve();
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {showConnections && (
        <BankConnections
          accounts={accounts}
          onClose={() => {
            setShowConnections(false);
            loadConnections();
          }}
        />
      )}
      {mappingFile && (
        <BankColumnMapping
          path={mappingFile.path}
          inspection={mappingFile.inspection}
          bankAccountId={bankAccountId}
          accountName={accountLabel(accounts, bankAccountId)}
          onCancel={() => setMappingFile(null)}
          onImport={async (mapping) => {
            const r = await api.importBankFile(mappingFile.path, bankAccountId, mapping);
            reportImport(r, mappingFile.path);
            setMappingFile(null);
          }}
        />
      )}
      <div className="qb-panel flex flex-none flex-wrap items-center gap-2 px-2 py-1">
        <label className="flex items-center gap-1.5">
          <span className="text-sm font-semibold text-qb-ink-muted">Account</span>
          <AccountSelect
            accounts={accounts}
            filter={isFeedAccount}
            value={bankAccountId}
            onChange={setBankAccountId}
            className="qb-input w-[240px]"
          />
        </label>
        <button type="button" className="qb-btn qb-btn-secondary" onClick={importStatement} disabled={busy || bankAccountId === null}>
          Import statement…
        </button>
        <span className="mx-1 h-4 w-px bg-qb-border" aria-hidden="true" />
        {syncable.length > 0 && (
          <button type="button" className="qb-btn qb-btn-primary" onClick={syncAll} disabled={busy}>
            {busy ? 'Working…' : 'Sync banks'}
          </button>
        )}
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setShowRules(true)} disabled={busy} title="Renaming rules for payees, and the category rules learned from your approvals">
          Rules…
        </button>
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setShowConnections(true)} disabled={busy}>
          {syncable.length > 0 ? 'Bank connections…' : 'Connect a bank…'}
        </button>
        <span className="text-xs text-qb-ink-muted">Statements: CSV, OFX or QFX · already-imported transactions are skipped</span>
        <div className="ml-auto flex gap-3 text-sm">
          {feedAccounts.map((a) => (
            <span key={a.id} className={a.id === bankAccountId ? 'font-semibold' : 'text-qb-ink-muted'}>
              {a.fullName}: {pendingByAccount.get(a.id) ?? 0} for review
            </span>
          ))}
        </div>
      </div>

      {showRules && <BankRules accounts={accounts} onClose={() => setShowRules(false)} />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {offer && (
        <div className="qb-panel flex flex-none flex-wrap items-center gap-1 px-2 py-1 text-sm" role="region" aria-label="Offer a renaming rule">
          <span className="font-semibold">Use this name next time?</span>
          <span className="text-qb-ink-muted">When a downloaded description</span>
          <RuleFields rule={offer} onChange={setOffer} />
          <button
            type="button"
            className="qb-btn qb-btn-primary h-row"
            onClick={() =>
              run(async () => {
                await api.saveBankPayeeRule({ id: 0, matchKind: offer.matchKind, pattern: offer.pattern, payeeName: offer.payeeName });
                setOffer(null);
              }, `Saved: descriptions like "${offer.description}" will be named ${offer.payeeName.trim()}.`)
            }
            disabled={busy}
          >
            Save rule
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setOffer(null)}>
            No thanks
          </button>
        </div>
      )}
      {error && <Notice kind="error" text={`Could not load bank lines: ${error}`} />}

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,3fr)_minmax(320px,2fr)] gap-2">
        <section className="qb-panel flex min-h-0 flex-col">
          <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
            <h2 className="text-md font-semibold">For review</h2>
            <span className="ml-2 text-xs text-qb-ink-muted">{pending.length} imported line{pending.length === 1 ? '' : 's'} · ↑↓ to move · Enter to approve</span>
            <div className="ml-auto flex items-center gap-2">
              <label className="flex items-center gap-1 text-xs" title="Also list the lines matched to transactions already entered">
                <input type="checkbox" className="accent-qb-green" checked={showMatched} onChange={(e) => setShowMatched(e.target.checked)} />
                Show matched ({matchedHere.length})
              </label>
              <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={addTicked} disabled={busy || tickedLines.length === 0} title="Add every ticked line with its category">
                Add {tickedLines.length || ''} ticked
              </button>
            </div>
          </div>
          <div ref={listRef} tabIndex={0} onKeyDown={onListKey} className="min-h-0 flex-1 overflow-auto outline-none scrollbar-thin focus-visible:shadow-focus">
            <table className="qb-grid">
              <colgroup>
                <col className="w-[30px]" />
                <col className="w-[92px]" />
                <col />
                <col className="w-[104px]" />
                <col className="w-[104px]" />
                <col className="w-[220px]" />
              </colgroup>
              <thead>
                <tr>
                  <th className="qb-grid-head text-center">
                    <input
                      type="checkbox"
                      className="accent-qb-green"
                      aria-label="Tick every line"
                      checked={pending.length > 0 && tickedLines.length === pending.length}
                      onChange={(e) => setTicked(e.target.checked ? new Set(pending.map((t) => t.id)) : new Set())}
                    />
                  </th>
                  <th className="qb-grid-head">Date</th>
                  <th className="qb-grid-head">Description</th>
                  <th className="qb-grid-head text-right">Spent</th>
                  <th className="qb-grid-head text-right">Received</th>
                  <th className="qb-grid-head">Suggested category</th>
                </tr>
              </thead>
              <tbody>
                {pending.length === 0 && (
                  <tr>
                    <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">
                      Nothing to review. Import a statement file from your bank, or connect a bank to sync transactions automatically.
                    </td>
                  </tr>
                )}
                {pending.map((t) => {
                  const chosen = overrides[t.id] ?? t.suggestion?.accountId ?? null;
                  return (
                    <tr
                      key={t.id}
                      data-id={t.id}
                      aria-selected={t.id === selectedId}
                      className="qb-grid-row"
                      onMouseDown={() => setSelectedId(t.id)}
                    >
                      <td className="qb-grid-cell text-center">
                        <input type="checkbox" className="accent-qb-green" checked={ticked.has(t.id)} onChange={() => toggleTick(t.id)} onMouseDown={(e) => e.stopPropagation()} aria-label={`Tick ${t.description}`} />
                      </td>
                      <td className="qb-grid-cell">{formatDate(t.date)}</td>
                      <td className="qb-grid-cell" title={t.description}>{t.description}</td>
                      <td className="qb-grid-cell num">{t.amountCents < 0 ? formatCents(-t.amountCents) : ''}</td>
                      <td className="qb-grid-cell num">{t.amountCents > 0 ? formatCents(t.amountCents) : ''}</td>
                      <td className={`qb-grid-cell ${t.suggestion?.origin === 'DEFAULT' ? 'text-qb-ink-faint' : ''}`}>
                        <div className="flex min-w-0 items-center">
                          {t.suggestedMatch ? (
                            <span className="min-w-0 truncate font-semibold text-qb-green-active" title={`Already entered: ${matchLabel(t.suggestedMatch)}`}>
                              Match: {matchLabel(t.suggestedMatch)}
                            </span>
                          ) : (
                            <span className="min-w-0 truncate">{chosen !== null ? accountLabel(accounts, chosen) : ''}</span>
                          )}
                          {!t.suggestedMatch && overrides[t.id] === undefined && <Confidence suggestion={t.suggestion} />}
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            {showMatched && (
              <table className="qb-grid mt-2" aria-label="Matched lines">
                <thead>
                  <tr>
                    <th className="qb-grid-head">Date</th>
                    <th className="qb-grid-head">Description</th>
                    <th className="qb-grid-head text-right">Amount</th>
                    <th className="qb-grid-head">Matched to</th>
                    <th className="qb-grid-head" aria-label="Unmatch" />
                  </tr>
                </thead>
                <tbody>
                  {matchedHere.length === 0 && (
                    <tr>
                      <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">No lines in this account are matched yet.</td>
                    </tr>
                  )}
                  {matchedHere.map((m) => (
                    <tr key={m.id} className="qb-grid-row">
                      <td className="qb-grid-cell">{formatDate(m.date)}</td>
                      <td className="qb-grid-cell" title={m.description}>{m.description}</td>
                      <td className="qb-grid-cell num">{formatCents(m.amountCents)}</td>
                      <td className="qb-grid-cell" title={m.origin === 'AUTO' ? 'Matched on import' : 'Matched by hand'}>
                        {matchLabel(m.matched)}
                      </td>
                      <td className="qb-grid-cell">
                        <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => unmatch(m)} disabled={busy}>
                          Unmatch
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </section>

        <section className="qb-panel flex min-h-0 flex-col">
          <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
            <h2 className="text-md font-semibold">Categorize</h2>
          </div>
          {selected ? (
            <div className={`flex flex-col gap-2 p-2 transition-colors duration-75 ${flashId ? 'bg-qb-green-tint' : ''}`}>
              <dl className="grid grid-cols-[88px_minmax(0,1fr)] gap-x-2 gap-y-0.5 text-sm">
                <dt className="text-qb-ink-muted">Date</dt>
                <dd>{formatDate(selected.date)}</dd>
                <dt className="text-qb-ink-muted">Description</dt>
                <dd className="break-words font-semibold">{selected.description}</dd>
                <dt className="text-qb-ink-muted">{selected.amountCents < 0 ? 'Spent' : 'Received'}</dt>
                <dd className="num text-left text-xl font-semibold">{formatCents(Math.abs(selected.amountCents))}</dd>
                <dt className="text-qb-ink-muted">Account</dt>
                <dd>{accountLabel(accounts, selected.bankAccountId)}</dd>
              </dl>

              <div className="rounded-xs border border-qb-border bg-qb-canvas px-2 py-1 text-sm">
                {selected.suggestedMatch ? (
                  <div className="flex items-center gap-2">
                    <span className="min-w-0 flex-1">
                      Already entered: <b>{matchLabel(selected.suggestedMatch)}</b>
                    </span>
                    <button type="button" className="qb-btn qb-btn-primary h-row" onClick={() => matchTo(selected, selected.suggestedMatch)} disabled={busy}>
                      Match
                    </button>
                  </div>
                ) : (
                  <div className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 text-qb-ink-muted">Already entered this one? Match it so it isn't added twice.</span>
                    <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={findMatches} disabled={busy}>
                      Find a match…
                    </button>
                  </div>
                )}
                {candidates?.id === selected.id && (
                  <ul className="mt-1 max-h-[160px] overflow-auto border-t border-qb-border pt-1">
                    {candidates.list.length === 0 && <li className="text-qb-ink-muted">Nothing entered in this account for this amount within four months.</li>}
                    {candidates.list.map((m) => (
                      <li key={m.journalEntryId} className="flex items-center gap-2 py-0.5">
                        <span className="min-w-0 flex-1 truncate">{matchLabel(m)}</span>
                        <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => matchTo(selected, m)} disabled={busy}>
                          Match
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>

              <label>
                <span className="qb-label">{selected.amountCents < 0 ? 'Expense category' : 'Income category'}</span>
                <AccountSelect
                  accounts={accounts}
                  filter={(a) => a.id !== selected.bankAccountId}
                  value={categoryId}
                  onChange={(id) => setOverrides((o) => ({ ...o, [selected.id]: id }))}
                  placeholder="Select category"
                />
                {selected.suggestion && overrides[selected.id] === undefined && (
                  <span className="mt-0.5 block text-xs text-qb-ink-muted">
                    {ORIGIN_TEXT[selected.suggestion.origin]}
                    <Confidence suggestion={selected.suggestion} />
                  </span>
                )}
                {selected.suggestion?.similarTo && overrides[selected.id] === undefined && (
                  <span className="block text-xs text-qb-ink-muted" data-testid="similar-to">
                    Because similar to “{selected.suggestion.similarTo.description}”,{' '}
                    {formatCents(Math.abs(selected.suggestion.similarTo.amountCents))} on {formatDate(selected.suggestion.similarTo.date)}
                  </span>
                )}
              </label>
              {selected.amountCents < 0 && (
                <label>
                  <span className="qb-label">Payee</span>
                  <input
                    className="qb-input w-full"
                    value={payee}
                    onChange={(e) => setPayees((p) => ({ ...p, [selected.id]: e.target.value }))}
                    list="bank-feed-vendors"
                    placeholder="Vendor (optional)"
                  />
                  <datalist id="bank-feed-vendors">
                    {(vendors ?? []).map((v) => (
                      <option key={v.id} value={v.name} />
                    ))}
                  </datalist>
                  {selected.payee && payees[selected.id] === undefined && (
                    <span className="mt-0.5 block text-xs text-qb-ink-muted">
                      {selected.payeeOrigin === 'RULE' ? 'Named by a renaming rule (Rules…)' : 'Suggested from similar past transactions'}
                    </span>
                  )}
                </label>
              )}
              {tracking.enabled && (
                <label>
                  <span className="qb-label">Class</span>
                  <ClassSelect classes={tracking.classes} value={classId} onChange={(id) => setClasses((c) => ({ ...c, [selected.id]: id }))} />
                </label>
              )}

              <div className="rounded-xs border border-qb-border bg-qb-canvas px-2 py-1 text-xs">
                <div className="mb-0.5 font-semibold text-qb-ink-muted">Journal entry preview</div>
                {categoryId !== null &&
                  (selected.amountCents < 0
                    ? [['Dr', categoryId], ['Cr', selected.bankAccountId]]
                    : [['Dr', selected.bankAccountId], ['Cr', categoryId]]
                  ).map(([side, id]) => (
                    <div key={side} className="flex gap-2">
                      <span className={`w-5 font-semibold ${side === 'Cr' ? 'pl-2' : ''}`}>{side}</span>
                      <span className="flex-1 truncate">{accountLabel(accounts, id)}</span>
                      <span className="num">{formatCents(Math.abs(selected.amountCents))}</span>
                    </div>
                  ))}
              </div>

              <div className="flex gap-2">
                <button type="button" className="qb-btn qb-btn-primary flex-1" onClick={approve} disabled={busy || categoryId === null}>
                  {busy ? 'Posting…' : 'Approve'}
                </button>
                <button type="button" className="qb-btn qb-btn-secondary" onClick={exclude} disabled={busy}>
                  Exclude
                </button>
              </div>
            </div>
          ) : (
            <div className="p-2 text-sm text-qb-ink-muted">Select a transaction on the left to categorize it.</div>
          )}
        </section>
      </div>
    </div>
  );
}
