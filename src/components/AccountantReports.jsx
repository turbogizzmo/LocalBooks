import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { ExportMenu, PERIODS, longDate, resolvePeriod } from './ReportParts.jsx';
import { ReportTable, reportExports } from './CustomReport.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { todayIso } from '../lib/dates.js';

/**
 * The Accountant & Taxes reports that are not presets of the report builder
 * (`accountant_reports.rs`): previous reconciliations, the reconciliation discrepancy check,
 * income tax summary and detail (with the tax line mapping), collections and open purchase orders.
 * Each comes back in the builder's shape, so it shows, prints and exports the same way.
 */
export const ACCOUNTANT_REPORTS = [
  ['prev-rec', 'Previous Reconciliation'],
  ['rec-discrepancy', 'Reconciliation Discrepancy'],
  ['income-tax-summary', 'Income Tax Summary'],
  ['income-tax-detail', 'Income Tax Detail'],
  ['collections', 'Collections Report'],
  ['open-pos', 'Open Purchase Orders'],
];

export const isAccountantReport = (id) => ACCOUNTANT_REPORTS.some(([key]) => key === id);

export default function AccountantReports({ report, navigate }) {
  if (report === 'prev-rec') return <PreviousReconciliation navigate={navigate} />;
  if (report === 'rec-discrepancy') return <Discrepancy navigate={navigate} />;
  if (report === 'income-tax-summary') return <IncomeTax key="summary" initialDetail={false} navigate={navigate} />;
  if (report === 'income-tax-detail') return <IncomeTax key="detail" initialDetail navigate={navigate} />;
  if (report === 'collections') return <AsOfReport key="collections" title="Collections Report" fetch={api.getCollectionsReport} empty="No invoices are past due." navigate={navigate} />;
  if (report === 'open-pos') return <AsOfReport key="open-pos" title="Open Purchase Orders" fetch={api.getOpenPurchaseOrdersReport} empty="No purchase orders are open." navigate={navigate} />;
  return null;
}

/** Opens what a row points at: its transaction, or an account's register. */
const openRow = (navigate, range) => (row) => {
  if (row.journalEntryId) return navigate?.('accounting/journal', 'open-entry', { entryId: row.journalEntryId });
  if (row.accountId) return navigate?.('accounting/register', 'open', { accountId: row.accountId, startDate: range?.[0] ?? null, endDate: range?.[1] ?? null });
  return undefined;
};

/** The toolbar's right end and the report itself, with its notes. */
function Shown({ title, subtitle, data, error, onRow, footer, empty, toolbar }) {
  const company = useCompany();
  const [notice, setNotice] = useState(null);
  const filename = title.replace(/[^A-Za-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'Report';
  const exports = data && reportExports({ company: company.name, title, subtitle, report: data, filename, onNotice: setNotice });
  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        {toolbar}
        <div className="ml-auto">{exports && <ExportMenu onNotice={setNotice} onCsv={exports.onCsv} onXlsx={exports.onXlsx} />}</div>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {data && (
        <ReportTable
          company={company}
          title={title}
          subtitle={subtitle}
          report={data}
          onRow={onRow}
          empty={empty}
          footer={[...data.notes, footer].filter(Boolean).join(' · ')}
        />
      )}
    </>
  );
}

// ------------------------------------------------------------------------------------------------
// Reconciliations
// ------------------------------------------------------------------------------------------------

function PreviousReconciliation({ navigate }) {
  const { data: recs, error: listError } = useLedgerQuery(api.listCompletedReconciliations);
  const accounts = useMemo(() => [...new Map((recs ?? []).map((r) => [r.accountId, r.accountName])).entries()], [recs]);
  const [accountId, setAccountId] = useState(null);
  const [recId, setRecId] = useState(null);
  const [detail, setDetail] = useState(true);
  const account = accountId ?? accounts[0]?.[0] ?? null;
  const statements = (recs ?? []).filter((r) => r.accountId === account);
  const chosen = statements.find((r) => r.id === recId) ?? statements[0] ?? null;
  const { data, error, reload } = useLedgerQuery(() => (chosen ? api.getPreviousReconciliationReport(chosen.id, detail) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [chosen?.id, detail, reload]);

  const toolbar = (
    <>
      <label>
        <span className="qb-label">Account</span>
        <select className="qb-input w-[220px]" value={account ?? ''} onChange={(e) => { setAccountId(Number(e.target.value)); setRecId(null); }} disabled={accounts.length === 0}>
          {accounts.map(([id, name]) => (
            <option key={id} value={id}>{name}</option>
          ))}
        </select>
      </label>
      <label>
        <span className="qb-label">Statement ending</span>
        <select className="qb-input w-[160px]" value={chosen?.id ?? ''} onChange={(e) => setRecId(Number(e.target.value))} disabled={statements.length === 0}>
          {statements.map((r) => (
            <option key={r.id} value={r.id}>{longDate(r.statementEndDate)}</option>
          ))}
        </select>
      </label>
      <fieldset className="flex items-end gap-2 self-end pb-0.5 text-sm">
        <legend className="sr-only">Report type</legend>
        <label className="flex items-center gap-1"><input type="radio" checked={detail} onChange={() => setDetail(true)} /> Detail</label>
        <label className="flex items-center gap-1"><input type="radio" checked={!detail} onChange={() => setDetail(false)} /> Summary</label>
      </fieldset>
    </>
  );
  if (recs && recs.length === 0) {
    return <div className="qb-panel p-3 text-sm text-qb-ink-muted">No reconciliation has been completed yet. Reconcile an account under Banking › Reconcile, then reprint it here.</div>;
  }
  return (
    <Shown
      title={detail ? 'Reconciliation Detail' : 'Reconciliation Summary'}
      subtitle={chosen ? `${chosen.accountName} · Statement ending ${longDate(chosen.statementEndDate)}` : ''}
      data={data}
      error={listError ?? error}
      onRow={openRow(navigate, null)}
      footer={detail ? 'click a line to open its transaction' : null}
      toolbar={toolbar}
    />
  );
}

function Discrepancy({ navigate }) {
  const { data: recs } = useLedgerQuery(api.listCompletedReconciliations);
  const accounts = useMemo(() => [...new Map((recs ?? []).map((r) => [r.accountId, r.accountName])).entries()], [recs]);
  const [accountId, setAccountId] = useState('');
  const { data, error, reload } = useLedgerQuery(() => api.getReconciliationDiscrepancyReport(accountId === '' ? null : Number(accountId)));
  useEffect(() => {
    reload();
  }, [accountId, reload]);
  const toolbar = (
    <label>
      <span className="qb-label">Account</span>
      <select className="qb-input w-[220px]" value={accountId} onChange={(e) => setAccountId(e.target.value)}>
        <option value="">All reconciled accounts</option>
        {accounts.map(([id, name]) => (
          <option key={id} value={id}>{name}</option>
        ))}
      </select>
    </label>
  );
  return (
    <Shown
      title="Reconciliation Discrepancy"
      subtitle="Cleared transactions changed, voided or deleted after their reconciliation was completed"
      data={data}
      error={error}
      onRow={openRow(navigate, null)}
      empty="No changes: every completed reconciliation still matches what was cleared."
      toolbar={toolbar}
    />
  );
}

// ------------------------------------------------------------------------------------------------
// Income tax
// ------------------------------------------------------------------------------------------------

const TAX_PERIODS = [{ id: 'lastFiscalYear', label: 'Last fiscal year' }, ...PERIODS.filter((p) => p.id !== 'lastFiscalYear')];
const lineTitle = (t) => `${t.line} · ${t.label}`;

function IncomeTax({ initialDetail, navigate }) {
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const { data: prefs } = useLedgerQuery(api.getPreferences);
  const { data: forms } = useLedgerQuery(api.listTaxForms);
  const { data: allLines } = useLedgerQuery(api.listTaxLines);
  const [period, setPeriod] = useState('lastFiscalYear');
  const [[start, end], setRange] = useState(() => resolvePeriod('lastFiscalYear', null));
  const [chosenForm, setForm] = useState(null);
  const [detail, setDetail] = useState(initialDetail);
  const [taxLine, setTaxLine] = useState('');
  const [mapping, setMapping] = useState(false);
  const form = chosenForm ?? prefs?.tax_form ?? null;
  const lines = (allLines ?? []).filter((t) => t.form === form);

  useEffect(() => {
    if (fiscal && period !== 'custom') setRange(resolvePeriod(period, fiscal));
  }, [fiscal]); // eslint-disable-line react-hooks/exhaustive-deps

  const valid = Boolean(form && start && end && start <= end);
  const { data, error, reload } = useLedgerQuery(() => (valid ? api.getIncomeTaxReport(start, end, detail, form, detail && taxLine ? taxLine : null) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [start, end, detail, form, taxLine, reload]);

  // A summary row opens that line's detail.
  const onRow = (row) => {
    if (detail) return openRow(navigate, [start, end])(row);
    const line = lines.find((t) => lineTitle(t) === row.cells[0]);
    if (!line) return undefined;
    setTaxLine(line.code);
    setDetail(true);
    return undefined;
  };
  const formName = (forms ?? []).find(([code]) => code === form)?.[1] ?? '';
  const toolbar = (
    <>
      <label>
        <span className="qb-label">Tax year</span>
        <select
          className="qb-input w-[160px]"
          value={period}
          onChange={(e) => {
            setPeriod(e.target.value);
            if (e.target.value !== 'custom') setRange(resolvePeriod(e.target.value, fiscal));
          }}
        >
          {TAX_PERIODS.map((p) => (
            <option key={p.id} value={p.id}>{p.label}</option>
          ))}
        </select>
      </label>
      <label>
        <span className="qb-label">From</span>
        <input type="date" className="qb-input w-[132px]" value={start} onChange={(e) => { setPeriod('custom'); setRange([e.target.value, end]); }} />
      </label>
      <label>
        <span className="qb-label">To</span>
        <input type="date" className="qb-input w-[132px]" value={end} onChange={(e) => { setPeriod('custom'); setRange([start, e.target.value]); }} aria-invalid={!valid} />
      </label>
      <label>
        <span className="qb-label">Tax form</span>
        <select className="qb-input w-[220px]" value={form ?? ''} onChange={(e) => { setForm(e.target.value); setTaxLine(''); }}>
          {(forms ?? []).map(([code, label]) => (
            <option key={code} value={code}>{label}</option>
          ))}
        </select>
      </label>
      <fieldset className="flex items-end gap-2 self-end pb-0.5 text-sm">
        <legend className="sr-only">Report type</legend>
        <label className="flex items-center gap-1"><input type="radio" checked={!detail} onChange={() => setDetail(false)} /> Summary</label>
        <label className="flex items-center gap-1"><input type="radio" checked={detail} onChange={() => setDetail(true)} /> Detail</label>
      </fieldset>
      {detail && (
        <label>
          <span className="qb-label">Tax line</span>
          <select className="qb-input w-[220px]" value={taxLine} onChange={(e) => setTaxLine(e.target.value)}>
            <option value="">All tax lines</option>
            {lines.map((t) => (
              <option key={t.code} value={t.code}>{lineTitle(t)}</option>
            ))}
          </select>
        </label>
      )}
      <button type="button" className={`qb-btn h-row ${mapping ? 'qb-btn-primary' : 'qb-btn-secondary'}`} aria-expanded={mapping} onClick={() => setMapping((m) => !m)}>
        Tax lines…
      </button>
    </>
  );
  return (
    <>
      <Shown
        title={detail ? 'Income Tax Detail' : 'Income Tax Summary'}
        subtitle={valid ? `${formName} · ${longDate(start)} – ${longDate(end)}` : ''}
        data={data}
        error={error}
        onRow={onRow}
        footer={detail ? 'click a line to open its transaction' : 'click a tax line to see its transactions'}
        empty="Nothing posted in these dates."
        toolbar={toolbar}
      />
      {mapping && <TaxLineMapping form={form} formName={formName} allLines={allLines ?? []} onClose={() => setMapping(false)} />}
    </>
  );
}

/** Assigns each account its tax line, for the chosen form. */
function TaxLineMapping({ form, formName, allLines, onClose }) {
  const { data: accounts, error } = useLedgerQuery(api.listAccountTaxLines);
  const [notice, setNotice] = useState(null);
  const [showAll, setShowAll] = useState(false);
  const balanceSheet = (a) => ['ASSET', 'LIABILITY', 'EQUITY'].includes(a.classification);
  const hasScheduleL = allLines.some((t) => t.form === form && t.kind === 'balanceSheet');
  // Income and expense accounts always; balance sheet ones for a form with Schedule L, when asked or already assigned.
  const shown = (accounts ?? []).filter((a) => (a.isActive || a.taxLine) && (!balanceSheet(a) || (hasScheduleL && (showAll || a.taxLine))));
  const choices = (a) => allLines.filter((t) => t.form === form && (t.kind === 'balanceSheet') === balanceSheet(a));
  const label = (code) => {
    const t = allLines.find((x) => x.code === code);
    return t ? `${lineTitle(t)}${t.form === form ? '' : ` (${t.form === 'SCHEDULE_C' ? 'Schedule C' : `Form ${t.form}`})`}` : `${code} (imported)`;
  };
  const save = async (a, code) => {
    try {
      await api.setAccountTaxLine(a.accountId, code || null);
      setNotice(null);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  return (
    <section className="qb-panel no-print flex-none">
      <div className="flex flex-wrap items-center gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="text-md font-semibold">Tax lines · {formName}</h2>
        <span className="text-xs text-qb-ink-muted">Pick the return line each account’s amounts go on. Saved as you choose.</span>
        {hasScheduleL && (
          <label className="flex items-center gap-1 text-sm">
            <input type="checkbox" checked={showAll} onChange={(e) => setShowAll(e.target.checked)} /> Show balance sheet accounts
          </label>
        )}
        <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={onClose}>Close</button>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      <div className="max-h-[360px] overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <colgroup>
            <col />
            <col className="w-[160px]" />
            <col className="w-[360px]" />
          </colgroup>
          <thead>
            <tr>
              <th className="qb-grid-head">Account</th>
              <th className="qb-grid-head">Type</th>
              <th className="qb-grid-head">Tax line</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((a) => {
              const options = choices(a);
              const known = !a.taxLine || options.some((t) => t.code === a.taxLine);
              return (
                <tr key={a.accountId} className="qb-grid-row">
                  <td className="qb-grid-cell">{a.accountName}</td>
                  <td className="qb-grid-cell text-qb-ink-muted">{a.classification.charAt(0) + a.classification.slice(1).toLowerCase()}</td>
                  <td className="qb-grid-cell-edit">
                    <select value={a.taxLine ?? ''} onChange={(e) => save(a, e.target.value)} aria-label={`Tax line for ${a.accountName}`}>
                      <option value="">Unassigned</option>
                      {!known && <option value={a.taxLine}>{label(a.taxLine)}</option>}
                      {options.map((t) => (
                        <option key={t.code} value={t.code}>{lineTitle(t)}</option>
                      ))}
                    </select>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

// ------------------------------------------------------------------------------------------------
// Collections and open purchase orders
// ------------------------------------------------------------------------------------------------

function AsOfReport({ title, fetch, empty, navigate }) {
  const [asOf, setAsOf] = useState(todayIso());
  const { data, error, reload } = useLedgerQuery(() => (asOf ? fetch(asOf) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [asOf, reload]);
  return (
    <Shown
      title={title}
      subtitle={asOf ? `As of ${longDate(asOf)}` : ''}
      data={data}
      error={error}
      onRow={openRow(navigate, null)}
      empty={empty}
      toolbar={
        <label>
          <span className="qb-label">As of</span>
          <input type="date" className="qb-input w-[132px]" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
        </label>
      }
    />
  );
}
