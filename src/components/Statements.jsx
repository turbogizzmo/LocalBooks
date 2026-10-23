import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { DocHeader, PrintPortal, useCompany } from './PrintDocuments.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { api, errorText, printDocument, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate, periodRange } from '../lib/dates.js';

const AGING = ['Current', '1–30 days past due', '31–60 days', '61–90 days', 'Over 90 days'];

const kindLabel = (row) => (row.sourceType === 'JOURNAL' ? 'Refund' : (SOURCE_LABEL[row.sourceType] ?? row.sourceType));

/** One customer statement, as printed. */
function CustomerStatementDoc({ company, s }) {
  return (
    <article className="doc doc-page-break">
      <DocHeader company={company} title="Statement" subtitle={`${formatDate(s.startDate)} – ${formatDate(s.endDate)}`} />
      <div className="doc-bill-to">
        <div className="doc-label">To</div>
        <div className="doc-company-name">{s.customerName}</div>
        {s.companyName && <div>{s.companyName}</div>}
        {s.billingAddress && <div className="doc-company-lines">{s.billingAddress}</div>}
      </div>
      <table className="doc-table">
        <thead>
          <tr>
            <th>Date</th>
            <th>Transaction</th>
            <th>Details</th>
            <th className="doc-num">Amount</th>
            <th className="doc-num">Balance</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>{formatDate(s.startDate)}</td>
            <td>Balance forward</td>
            <td />
            <td />
            <td className="doc-num">{formatCents(s.openingBalanceCents)}</td>
          </tr>
          {s.rows.map((r, i) => (
            <tr key={i}>
              <td>{formatDate(r.date)}</td>
              <td>
                {kindLabel(r)}
                {r.number ? ` #${r.number}` : ''}
              </td>
              <td>{r.partyName !== s.customerName ? r.partyName.slice(s.customerName.length + 1) : (r.memo ?? '')}</td>
              <td className="doc-num">{formatCents(r.amountCents)}</td>
              <td className="doc-num">{formatCents(r.balanceCents)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <table className="doc-totals">
        <tbody>
          <tr className="doc-grand">
            <td>Amount due</td>
            <td className="doc-num">{formatCents(s.closingBalanceCents)}</td>
          </tr>
        </tbody>
      </table>
      <table className="doc-table">
        <thead>
          <tr>
            {AGING.map((a) => (
              <th key={a} className="doc-num">{a}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          <tr>
            {s.agingCents.map((c, i) => (
              <td key={i} className="doc-num">{formatCents(c)}</td>
            ))}
          </tr>
        </tbody>
      </table>
    </article>
  );
}

/** Sales › Customer Statements: pick a period and customers, preview, and print or save as PDF. */
export default function Statements() {
  const company = useCompany();
  const [[start, end], setRange] = useState(() => periodRange('lastMonth'));
  const customers = useLedgerQuery(() => api.listStatementCustomers(start, end));
  const [picked, setPicked] = useState(() => new Set());
  const [preview, setPreview] = useState(null);
  const [printing, setPrinting] = useState(null);
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (start && end && start <= end) customers.reload();
    setPicked(new Set());
    setPreview(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [start, end]);

  const list = customers.data ?? [];
  const toggle = (id) =>
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const show = async (id) => {
    try {
      setPreview(await api.getCustomerStatement(id, start, end));
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const print = async () => {
    try {
      const statements = await Promise.all([...picked].map((id) => api.getCustomerStatement(id, start, end)));
      setPrinting(statements);
      setTimeout(() => printDocument(), 120);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printing && (
        <PrintPortal>
          {printing.map((s) => (
            <CustomerStatementDoc key={s.customerId} company={company} s={s} />
          ))}
        </PrintPortal>
      )}
      <div className="qb-panel flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">Statement period from</span>
          <input type="date" className="qb-input w-[132px]" value={start} onChange={(e) => setRange([e.target.value, end])} />
        </label>
        <label>
          <span className="qb-label">To (statement date)</span>
          <input type="date" className="qb-input w-[132px]" value={end} onChange={(e) => setRange([start, e.target.value])} aria-invalid={end < start} />
        </label>
        <span className="self-center text-xs text-qb-ink-muted">Customers with a balance or activity in the period. A customer’s statement includes its jobs.</span>
        <button type="button" className="qb-btn qb-btn-primary ml-auto" onClick={print} disabled={picked.size === 0}>
          Print {picked.size || ''} statement{picked.size === 1 ? '' : 's'}
        </button>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {customers.error && <Notice kind="error" text={customers.error} />}
      <div className="grid min-h-0 flex-1 grid-cols-[320px_minmax(0,1fr)] gap-2">
        <section className="qb-panel flex min-h-0 flex-col">
          <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
            <input type="checkbox" checked={list.length > 0 && picked.size === list.length} onChange={(e) => setPicked(e.target.checked ? new Set(list.map((c) => c[0])) : new Set())} aria-label="Select all" />
            <h2 className="text-md font-semibold">Customers</h2>
          </div>
          <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
            <table className="qb-grid">
              <tbody>
                {customers.data && list.length === 0 && (
                  <tr>
                    <td className="qb-grid-cell text-qb-ink-muted">No customers with a balance or activity in this period.</td>
                  </tr>
                )}
                {list.map(([id, name, balance]) => (
                  <tr key={id} className={`qb-grid-row cursor-pointer ${preview?.customerId === id ? 'is-active' : ''}`} onClick={() => show(id)}>
                    <td className="qb-grid-cell w-[28px] text-center" onClick={(e) => e.stopPropagation()}>
                      <input type="checkbox" checked={picked.has(id)} onChange={() => toggle(id)} aria-label={`Select ${name}`} />
                    </td>
                    <td className="qb-grid-cell font-semibold">{name}</td>
                    <td className="qb-grid-cell num">{formatCents(balance)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
        <section className="qb-panel min-h-0 overflow-auto scrollbar-thin p-3">
          {!preview ? (
            <div className="text-sm text-qb-ink-muted">Click a customer to preview their statement.</div>
          ) : (
            <div className="mx-auto max-w-[760px] text-sm">
              <div className="mb-2 flex items-start justify-between border-b-[3px] border-double border-qb-ink pb-2">
                <div>
                  <div className="text-xs uppercase tracking-wide text-qb-ink-muted">{company.name}</div>
                  <h2 className="text-xl font-semibold">Statement · {preview.customerName}</h2>
                  <div className="text-qb-ink-muted">{formatDate(preview.startDate)} – {formatDate(preview.endDate)}</div>
                </div>
                <div className="text-right">
                  <div className="qb-label">Amount due</div>
                  <div className="num text-2xl font-semibold">{formatCents(preview.closingBalanceCents)}</div>
                </div>
              </div>
              <table className="w-full">
                <thead>
                  <tr className="border-b border-qb-border text-left text-xs uppercase text-qb-ink-muted">
                    <th className="py-0.5">Date</th>
                    <th>Transaction</th>
                    <th>Details</th>
                    <th className="text-right">Amount</th>
                    <th className="text-right">Balance</th>
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    <td className="py-0.5">{formatDate(preview.startDate)}</td>
                    <td>Balance forward</td>
                    <td />
                    <td />
                    <td className="num text-right">{formatCents(preview.openingBalanceCents)}</td>
                  </tr>
                  {preview.rows.map((r, i) => (
                    <tr key={i} className="border-t border-qb-border">
                      <td className="py-0.5">{formatDate(r.date)}</td>
                      <td>
                        {kindLabel(r)}
                        {r.number ? ` #${r.number}` : ''}
                      </td>
                      <td className="text-qb-ink-muted">{r.partyName !== preview.customerName ? r.partyName.slice(preview.customerName.length + 1) : (r.memo ?? '')}</td>
                      <td className="num text-right">{formatCents(r.amountCents)}</td>
                      <td className="num text-right font-semibold">{formatCents(r.balanceCents)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="mt-3 grid grid-cols-5 gap-2 border-t border-qb-border pt-2 text-xs">
                {AGING.map((a, i) => (
                  <div key={a}>
                    <div className="text-qb-ink-muted">{a}</div>
                    <div className="num font-semibold">{formatCents(preview.agingCents[i])}</div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
