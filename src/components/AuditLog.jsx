import { useCallback, useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { ExportMenu, tableExports } from './ReportParts.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { askConfirm } from '../lib/dialogs.jsx';
import { formatCents } from '../lib/money.js';
import { todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';

const ACTION_STYLE = {
  POST: { label: 'Posted', className: 'text-qb-ink' },
  VOID: { label: 'Voided', className: 'font-semibold text-qb-negative' },
  LOCK_RECONCILIATION: { label: 'Reconciled', className: 'font-semibold text-qb-green-active' },
};

const TIMESTAMP = new Intl.DateTimeFormat('en-US', { dateStyle: 'short', timeStyle: 'medium' });

/** A recorded UTC instant as local "YYYY-MM-DD HH:MM:SS", the form an Excel date-time cell takes. */
const localStamp = (iso) => {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
};

const STATUS_STYLE = {
  FIXABLE: { label: 'Correctable', className: 'text-qb-info' },
  CORRECTED: { label: 'Corrected', className: 'text-qb-green-active' },
  OUT_OF_BALANCE: { label: 'Restore a backup', className: 'text-qb-negative' },
  MISSING_ACCOUNT: { label: 'Restore a backup', className: 'text-qb-negative' },
  NO_BALANCE_EFFECT: { label: 'Review entry', className: 'text-qb-ink-muted' },
};

/** Reviewed corrections: each proposed entry is shown in full and posts only when approved. */
function Corrections({ onChanged }) {
  const [proposals, setProposals] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(null);

  const load = useCallback(async () => {
    try {
      setProposals(await api.proposeForensicCorrections());
    } catch (err) {
      setError(errorText(err));
    }
  }, []);
  useEffect(() => {
    load();
  }, [load]);

  const apply = async (p) => {
    const label = p.entryNumber ? `#${p.entryNumber}` : `id ${p.entryId}`;
    if (!(await askConfirm(`Post a correcting entry dated today for entry ${label}? The altered rows stay in the ledger as evidence.`, { confirmLabel: 'Post Correction' }))) return;
    setBusy(p.entryId);
    setError(null);
    try {
      await api.applyForensicCorrection({ entryId: p.entryId, lines: p.lines, date: todayIso() });
      await load();
      onChanged();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  };

  if (error) return <Notice kind="error" text={error} onDismiss={() => setError(null)} />;
  if (!proposals) return null;
  return (
    <div className="flex max-h-[320px] flex-col gap-1 overflow-auto border border-qb-border bg-white p-2 text-xs scrollbar-thin">
      <p className="text-qb-ink-muted">
        Each correction is a new, balanced entry that restores every account to the amount recorded when the original entry was posted.
        Nothing posts until you approve it. Changes that left the books out of balance cannot be corrected with an entry: restore a backup
        taken before the change (Settings › File &amp; Data Controls).
      </p>
      {proposals.map((p) => (
        <div key={p.entryId} className="border-t border-qb-border pt-1">
          <div className="flex items-center gap-2">
            <span className="font-semibold">Entry #{p.entryNumber ?? `id ${p.entryId}`}{p.entryDeleted ? ' (deleted)' : ''}</span>
            <span className={`font-semibold ${STATUS_STYLE[p.status]?.className ?? ''}`}>{STATUS_STYLE[p.status]?.label ?? p.status}</span>
            <span className="text-qb-ink-muted">{p.explanation}</span>
            {p.status === 'FIXABLE' && (
              <button type="button" className="qb-btn qb-btn-primary ml-auto h-row flex-none px-2" disabled={busy !== null} onClick={() => apply(p)}>
                {busy === p.entryId ? 'Posting…' : 'Post correction'}
              </button>
            )}
          </div>
          {p.lines.length > 0 && (
            <table className="qb-grid mt-1">
              <thead>
                <tr>
                  <th className="qb-grid-head">Account</th>
                  <th className="qb-grid-head text-right">Debit</th>
                  <th className="qb-grid-head text-right">Credit</th>
                </tr>
              </thead>
              <tbody>
                {p.lines.map((l) => (
                  <tr key={l.accountId}>
                    <td className="qb-grid-cell">{l.accountName}</td>
                    <td className="qb-grid-cell num">{l.debitCents ? formatCents(l.debitCents) : ''}</td>
                    <td className="qb-grid-cell num">{l.creditCents ? formatCents(l.creditCents) : ''}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      ))}
    </div>
  );
}

/** Result of comparing every posted entry with the snapshot recorded when it was posted. */
function Verification({ result, onDismiss, onChanged }) {
  const [reviewing, setReviewing] = useState(false);
  if (result.error) return <Notice kind="error" text={`Verification could not run: ${result.error}`} onDismiss={onDismiss} />;
  const { entriesChecked, voidsChecked, migrationsRecorded, auditRecordsSealed, auditTrailRecordedSince, findings } = result.data;
  if (findings.length === 0) {
    const chain = auditTrailRecordedSince
      ? `${auditRecordsSealed.toLocaleString()} audit records are linked and consistent, and match the record this computer has kept of this file's audit trail since ${auditTrailRecordedSince} (changes made outside the app before then can't be compared).`
      : `${auditRecordsSealed.toLocaleString()} audit records are linked and consistent; this computer has no record of this file's audit trail to compare against.`;
    return (
      <Notice
        kind="success"
        text={`✓ Ledger intact: all ${entriesChecked.toLocaleString()} posted entries match their posting snapshots, ${voidsChecked.toLocaleString()} voids link to their reversals, ${migrationsRecorded} schema migrations recorded. ${chain}`}
        onDismiss={onDismiss}
      />
    );
  }
  return (
    <div className="flex flex-col gap-1">
      <Notice kind="error" text={`${findings.length} integrity problem${findings.length === 1 ? '' : 's'} found in ${entriesChecked.toLocaleString()} posted entries.`} onDismiss={onDismiss} />
      <ul className="max-h-[120px] overflow-auto border border-qb-negative/30 bg-white px-2 py-1 text-xs scrollbar-thin">
        {findings.map((f) => (
          <li key={`${f.auditRecordId ?? ''}-${f.entryId}-${f.problem}`}>
            <span className="font-semibold">{f.auditRecordId ? `Audit record ${f.auditRecordId}` : `Entry #${f.entryNumber ?? `id ${f.entryId}`}`}</span>: {f.problem}
          </li>
        ))}
      </ul>
      {reviewing ? (
        <Corrections onChanged={onChanged} />
      ) : (
        <button type="button" className="qb-btn qb-btn-secondary h-row self-start px-2 text-xs" onClick={() => setReviewing(true)}>
          Review corrections…
        </button>
      )}
    </div>
  );
}

/** Read-only, database-enforced history of every post, void and reconciliation lock. */
export default function AuditLog() {
  const { data, error } = useLedgerQuery(api.listAuditLog);
  const [action, setAction] = useState('ALL');
  const [verification, setVerification] = useState(null);
  const [verifying, setVerifying] = useState(false);

  const verify = async () => {
    setVerifying(true);
    try {
      setVerification({ data: await api.verifyLedgerIntegrity() });
    } catch (err) {
      setVerification({ error: errorText(err) });
    } finally {
      setVerifying(false);
    }
  };
  const sorter = useSort((data ?? []).filter((a) => action === 'ALL' || a.action === action), {
    when: (a) => a.occurredAt,
    event: (a) => ACTION_STYLE[a.action]?.label ?? a.action,
    source: (a) => (a.sourceType ? (SOURCE_LABEL[a.sourceType] ?? a.sourceType) : null),
    entry: (a) => a.entryNumber,
    detail: (a) => a.detail,
    amount: (a) => a.amountCents,
  }, { initial: { key: 'when', dir: 'desc' }, storageKey: 'audit-log' });
  const rows = sorter.sorted;
  const company = useCompany();
  const [exportNotice, setExportNotice] = useState(null);
  const exports = tableExports({
    company: company.name,
    title: 'Audit Log',
    subtitle: action === 'ALL' ? 'All events' : `${ACTION_STYLE[action]?.label ?? action} events`,
    columns: [
      { label: 'When', kind: 'datetime' },
      { label: 'Event', kind: 'text' },
      { label: 'Source', kind: 'text' },
      { label: 'Entry', kind: 'text' },
      { label: 'Detail', kind: 'text' },
      { label: 'Amount', kind: 'money' },
    ],
    rows: () =>
      rows.map((a) => ({
        cells: [
          a.occurredAt ? localStamp(a.occurredAt) : '',
          ACTION_STYLE[a.action]?.label ?? a.action,
          a.sourceType ? (SOURCE_LABEL[a.sourceType] ?? a.sourceType) : '',
          a.entryNumber == null ? '' : String(a.entryNumber),
          a.detail ?? '',
          a.amountCents,
        ],
      })),
    footer: 'Recorded by the database itself · times are this computer\'s local time',
    filename: `Audit-Log_${todayIso()}`,
    onNotice: setExportNotice,
  });

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {error && <Notice kind="error" text={`Could not load the audit log: ${error}`} />}
      {exportNotice && <Notice kind={exportNotice.kind} text={exportNotice.text} onDismiss={() => setExportNotice(null)} />}
      {verification && <Verification result={verification} onDismiss={() => setVerification(null)} onChanged={verify} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Audit log</h2>
          <select className="qb-input h-row w-[160px] text-sm" value={action} onChange={(e) => setAction(e.target.value)} aria-label="Action">
            <option value="ALL">All events</option>
            <option value="POST">Posted</option>
            <option value="VOID">Voided</option>
            <option value="LOCK_RECONCILIATION">Reconciliations</option>
          </select>
          <span className="ml-auto text-xs text-qb-ink-muted">Recorded by the database itself · cannot be edited or deleted</span>
          <button type="button" className="qb-btn-secondary" onClick={verify} disabled={verifying}>
            {verifying ? 'Verifying…' : 'Verify ledger integrity'}
          </button>
          <ExportMenu printable={false} onNotice={setExportNotice} {...exports} />
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[170px]" />
              <col className="w-[96px]" />
              <col className="w-[120px]" />
              <col className="w-[64px]" />
              <col />
              <col className="w-[120px]" />
            </colgroup>
            <thead>
              <tr>
                <SortTh sorter={sorter} col="when">When</SortTh>
                <SortTh sorter={sorter} col="event">Event</SortTh>
                <SortTh sorter={sorter} col="source">Source</SortTh>
                <SortTh sorter={sorter} col="entry">Entry</SortTh>
                <SortTh sorter={sorter} col="detail">Detail</SortTh>
                <SortTh sorter={sorter} col="amount" className="text-right">Amount</SortTh>
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">Nothing recorded yet.</td>
                </tr>
              )}
              {rows.map((a) => (
                <tr key={a.id} className="qb-grid-row">
                  <td className="qb-grid-cell text-qb-ink-muted">{a.occurredAt ? TIMESTAMP.format(new Date(a.occurredAt)) : ''}</td>
                  <td className={`qb-grid-cell ${ACTION_STYLE[a.action]?.className ?? ''}`}>{ACTION_STYLE[a.action]?.label ?? a.action}</td>
                  <td className="qb-grid-cell">{a.sourceType ? (SOURCE_LABEL[a.sourceType] ?? a.sourceType) : ''}</td>
                  <td className="qb-grid-cell">{a.entryNumber ?? ''}</td>
                  <td className="qb-grid-cell" title={a.detail}>{a.detail}</td>
                  <td className="qb-grid-cell num">{a.amountCents !== null ? formatCents(a.amountCents) : ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
