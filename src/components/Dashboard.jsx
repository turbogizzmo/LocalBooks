import { useState } from 'react';
import ArAgingWidget from './ArAgingWidget.jsx';
import AssetLiabilityChart from './AssetLiabilityChart.jsx';
import HomeFlow from './HomeFlow.jsx';
import Notice from './Notice.jsx';
import RemindersPanel from './Reminders.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { CashForecastWidget } from './CashForecast.jsx';
import { Trend } from './CompanySnapshot.jsx';
import { periodRange } from '../lib/dates.js';

function StatTile({ label, cents, emphasis }) {
  return (
    <div className="qb-panel px-2 py-1">
      <div className="text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">{label}</div>
      <div className={`num text-2xl font-semibold ${emphasis && cents < 0 ? 'num-negative' : ''}`}>
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
        <table className="qb-grid min-w-[400px]">
          <colgroup>
            <col className="w-[56px]" />
            <col />
            {/* The type column only where the panel is wide enough for it and full amounts. */}
            <col className="hidden w-[150px] 2xl:table-column" />
            <col className="w-[128px]" />
            <col className="w-[128px]" />
          </colgroup>
          <thead>
            <tr>
              <th className="qb-grid-head">No.</th>
              <th className="qb-grid-head">Account</th>
              <th className="qb-grid-head hidden 2xl:table-cell">Type</th>
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
                <td className="qb-grid-cell hidden text-qb-ink-muted 2xl:table-cell">{row.accountTypeName}</td>
                <td className="qb-grid-cell num">{row.debitCents ? formatCents(row.debitCents) : ''}</td>
                <td className="qb-grid-cell num">{row.creditCents ? formatCents(row.creditCents) : ''}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="sticky bottom-0 bg-qb-row-header font-semibold">
              <td className="qb-grid-cell" colSpan={2}>Total</td>
              <td className="qb-grid-cell hidden 2xl:table-cell" />
              <td className="qb-grid-cell num border-t-2 border-double border-qb-ink">{formatCents(tb.totalDebitCents)}</td>
              <td className="qb-grid-cell num border-t-2 border-double border-qb-ink">{formatCents(tb.totalCreditCents)}</td>
            </tr>
          </tfoot>
        </table>
      </div>
    </section>
  );
}

const TAB_KEY = 'localbooks.dashboardTab';
const TABS = [
  ['home', 'Home'],
  ['insights', 'Insights'],
];

function savedTab() {
  try {
    return localStorage.getItem(TAB_KEY) === 'insights' ? 'insights' : 'home';
  } catch {
    return 'home';
  }
}

/**
 * Income and expense by month for the last twelve months, on the Preferences report basis: the
 * same figures as the Income & Expense graph and each month's Profit and Loss.
 */
function IncomeExpenseTrend({ navigate }) {
  const [start, end] = periodRange('last12Months');
  const { data, error } = useLedgerQuery(async () => {
    const prefs = await api.getPreferences();
    return api.getIncomeExpenseGraph(start, end, prefs?.report_basis === 'cash' ? 'CASH' : 'ACCRUAL');
  });
  return (
    <section className="qb-panel flex flex-col px-2 py-1" aria-label="Income and expense trend">
      <div className="flex items-baseline gap-2">
        <h2 className="text-md font-semibold">Income and expense, last 12 months</h2>
        {data && (
          <span className="ml-auto text-xs text-qb-ink-muted">
            Income {formatCents(data.totalIncomeCents)} · Expenses {formatCents(data.totalExpenseCents)} · Net {formatCents(data.netIncomeCents)}
          </span>
        )}
      </div>
      {error && <Notice kind="error" text={error} />}
      {data && <Trend height={120} points={data.months.map((p) => ({ month: p.month, incomeCents: p.incomeCents, expenseCents: p.expenseCents, netCents: p.netIncomeCents }))} />}
      <button type="button" className="self-start text-xs text-qb-info hover:underline" onClick={() => navigate?.('reports', 'open', { report: 'graph-ie', dates: { startDate: start, endDate: end } })}>
        Open the Income &amp; Expense graph
      </button>
    </section>
  );
}

/** The Insights tab: totals, the income and expense trend, the assets and liabilities chart, the trial balance and reminders. */
function Insights({ navigate }) {
  const { data, error } = useLedgerQuery(api.getDashboard);

  if (error) return <Notice kind="error" text={`Could not load dashboard: ${error}`} />;
  if (!data) return <div className="px-2 py-1 text-sm text-qb-ink-muted">Loading…</div>;

  return (
    <>
      <div className="grid flex-none grid-cols-5 gap-2">
        <StatTile label="Total assets" cents={data.totalAssetsCents} />
        <StatTile label="Total liabilities" cents={data.totalLiabilitiesCents} />
        <StatTile label="Assets less liabilities" cents={data.totalAssetsCents - data.totalLiabilitiesCents} emphasis />
        <ArAgingWidget />
      </div>
      <div className="flex-none">
        <IncomeExpenseTrend navigate={navigate} />
      </div>
      <div className="flex-none">
        <CashForecastWidget navigate={navigate} />
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(260px,0.8fr)] gap-2">
        <AssetLiabilityChart series={data.series} />
        <TrialBalance tb={data.trialBalance} />
        <RemindersPanel navigate={navigate} />
      </div>
    </>
  );
}

/** The Dashboard: the Home map of the day-to-day work, and Insights. The tab chosen is remembered. */
export default function Dashboard({ accounts, navigate }) {
  const [tab, setTab] = useState(savedTab);
  const choose = (id) => {
    setTab(id);
    try {
      localStorage.setItem(TAB_KEY, id);
    } catch {
      // Only a convenience.
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="flex flex-none overflow-hidden self-start rounded-xs border border-qb-border-strong text-sm" role="tablist" aria-label="Dashboard">
        {TABS.map(([id, label]) => (
          <button key={id} type="button" role="tab" aria-selected={tab === id} onClick={() => choose(id)} className={`px-2.5 py-0.5 font-semibold ${tab === id ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}>
            {label}
          </button>
        ))}
      </div>
      {tab === 'home' ? <HomeFlow accounts={accounts} navigate={navigate} /> : <Insights navigate={navigate} />}
    </div>
  );
}
