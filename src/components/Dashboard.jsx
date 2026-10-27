import ArAgingWidget from './ArAgingWidget.jsx';
import AssetLiabilityChart from './AssetLiabilityChart.jsx';
import Notice from './Notice.jsx';
import RemindersPanel from './Reminders.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';

function StatTile({ label, cents, emphasis }) {
  return (
    <div className="qb-panel px-2 py-1">
      <div className="text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">{label}</div>
      <div className={`num text-left text-2xl font-semibold ${emphasis && cents < 0 ? 'num-negative' : ''}`}>
        {formatCents(cents)}
      </div>
    </div>
  );
}

function TrialBalance({ tb }) {
  const difference = tb.totalDebitCents - tb.totalCreditCents;
  return (
    <section className="qb-panel flex min-h-0 flex-col">
      <div className="flex h-row-lg flex-none items-center justify-between border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Trial Balance</h2>
        {difference === 0 ? (
          <span className="text-xs font-semibold text-qb-green-active">✓ In balance</span>
        ) : (
          <span className="text-xs font-semibold text-qb-negative">⚠ Out of balance by {formatCents(Math.abs(difference))}</span>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <colgroup>
            <col className="w-[64px]" />
            <col />
            <col className="w-[150px]" />
            <col className="w-[112px]" />
            <col className="w-[112px]" />
          </colgroup>
          <thead>
            <tr>
              <th className="qb-grid-head">No.</th>
              <th className="qb-grid-head">Account</th>
              <th className="qb-grid-head">Type</th>
              <th className="qb-grid-head text-right">Debit</th>
              <th className="qb-grid-head text-right">Credit</th>
            </tr>
          </thead>
          <tbody>
            {tb.rows.length === 0 && (
              <tr>
                <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">
                  No posted activity yet. Post an invoice or approve a bank line to populate the ledger.
                </td>
              </tr>
            )}
            {tb.rows.map((row) => (
              <tr key={row.accountId} className="qb-grid-row">
                <td className="qb-grid-cell text-qb-ink-muted">{row.accountNumber ?? ''}</td>
                <td className="qb-grid-cell" title={row.fullName}>{row.fullName}</td>
                <td className="qb-grid-cell text-qb-ink-muted">{row.accountTypeName}</td>
                <td className="qb-grid-cell num">{row.debitCents ? formatCents(row.debitCents) : ''}</td>
                <td className="qb-grid-cell num">{row.creditCents ? formatCents(row.creditCents) : ''}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="sticky bottom-0 bg-qb-row-header font-semibold">
              <td className="qb-grid-cell" colSpan={3}>Total</td>
              <td className="qb-grid-cell num border-t-2 border-double border-qb-ink">{formatCents(tb.totalDebitCents)}</td>
              <td className="qb-grid-cell num border-t-2 border-double border-qb-ink">{formatCents(tb.totalCreditCents)}</td>
            </tr>
          </tfoot>
        </table>
      </div>
    </section>
  );
}

export default function Dashboard({ navigate }) {
  const { data, error } = useLedgerQuery(api.getDashboard);

  if (error) return <Notice kind="error" text={`Could not load dashboard: ${error}`} />;
  if (!data) return <div className="px-2 py-1 text-sm text-qb-ink-muted">Loading…</div>;

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="grid flex-none grid-cols-5 gap-2">
        <StatTile label="Total assets" cents={data.totalAssetsCents} />
        <StatTile label="Total liabilities" cents={data.totalLiabilitiesCents} />
        <StatTile label="Assets less liabilities" cents={data.totalAssetsCents - data.totalLiabilitiesCents} emphasis />
        <ArAgingWidget />
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(260px,0.8fr)] gap-2">
        <AssetLiabilityChart series={data.series} />
        <TrialBalance tb={data.trialBalance} />
        <RemindersPanel navigate={navigate} />
      </div>
    </div>
  );
}
