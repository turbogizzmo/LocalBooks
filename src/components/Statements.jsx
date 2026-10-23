import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { FormSheet, useFormPrinter } from './FormPages.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { periodRange } from '../lib/dates.js';

/** Sales › Customer Statements: pick a period and customers, preview, and print or save as PDF. */
export default function Statements() {
  const [[start, end], setRange] = useState(() => periodRange('lastMonth'));
  const customers = useLedgerQuery(() => api.listStatementCustomers(start, end));
  const [picked, setPicked] = useState(() => new Set());
  const [preview, setPreview] = useState(null); // { customerId, form }
  const [notice, setNotice] = useState(null);
  const [printPortal, forms] = useFormPrinter(setNotice);
  const period = { startDate: start, endDate: end };

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
  // The statement as its customer's form style prints it.
  const show = async (id) => {
    try {
      setPreview({ customerId: id, form: await api.renderForm('STATEMENT', id, period) });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const print = () => forms.printMany([...picked].map((id) => ({ kind: 'STATEMENT', id, period })));

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printPortal}
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
        <section className="qb-panel flex min-h-0 flex-col">
          <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
            <h2 className="text-md font-semibold">{preview ? preview.form.title : 'Statement preview'}</h2>
            {preview && (
              <span className="ml-auto flex gap-1">
                <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => forms.print('STATEMENT', preview.customerId, period)}>
                  Print…
                </button>
                <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => forms.savePdf('STATEMENT', preview.customerId, period)}>
                  Save as PDF…
                </button>
              </span>
            )}
          </div>
          <div className="min-h-0 flex-1 overflow-auto bg-qb-canvas p-3 scrollbar-thin">
            {!preview ? (
              <div className="text-sm text-qb-ink-muted">Click a customer to preview their statement. It uses their form style (Settings › Custom Forms).</div>
            ) : (
              <div className="mx-auto flex max-w-[720px] flex-col gap-3">
                {preview.form.pages.map((page, i) => (
                  <FormSheet key={i} page={page} logo={preview.form.logo} className="w-full shadow-md" />
                ))}
              </div>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
