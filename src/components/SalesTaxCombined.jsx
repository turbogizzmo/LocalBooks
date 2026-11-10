import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { ratePercent } from './SalesTax.jsx';

/**
 * A combined rate (`sales_tax/combined.rs`): a name and the component rates it adds up, each with
 * its own agency and liability account. A sale at it posts one tax line per component.
 */
export function CombinedRateDialog({ rate, rates, onClose, onSaved }) {
  const [name, setName] = useState(rate?.name ?? '');
  const [chosen, setChosen] = useState(rate?.componentIds ?? []);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const parts = rates.filter((r) => !r.isGroup && (r.isActive || chosen.includes(r.id)));
  const total = chosen.reduce((sum, id) => sum + (rates.find((r) => r.id === id)?.ratePpm ?? 0), 0);
  const toggle = (id) => setChosen((c) => (c.includes(id) ? c.filter((x) => x !== id) : [...c, id]));
  const submit = async (e) => {
    e.preventDefault();
    setBusy(true);
    try {
      const saved = await api.saveCombinedRate(rate?.id ?? null, { name, componentIds: chosen });
      onSaved(`${rate ? 'Saved' : 'Added'} ${saved.name} at ${ratePercent(saved.ratePpm)}.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };
  return (
    <Dialog title={rate ? `Edit ${rate.name}` : 'New combined rate'} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <label className="flex flex-col gap-0.5">
          <span className="font-semibold text-qb-ink-muted">Name</span>
          <input className="qb-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Harbor City 8.5%" />
        </label>
        <fieldset className="flex flex-col gap-1">
          <legend className="font-semibold text-qb-ink-muted">Rates it adds up (each paid to its own agency)</legend>
          {parts.length < 2 && <p className="text-qb-ink-muted">Add the separate rates first (state, county, city…), each with its agency.</p>}
          {parts.map((r) => (
            <label key={r.id} className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={chosen.includes(r.id)} onChange={() => toggle(r.id)} />
              <span className="flex-1">{r.name}</span>
              <span className="text-qb-ink-muted">{r.agencyName ?? 'No agency'}</span>
              <span className="num w-[60px]">{ratePercent(r.ratePpm)}</span>
            </label>
          ))}
        </fieldset>
        <p className="text-sm">
          Combined rate: <span className="font-semibold">{ratePercent(total)}</span>
        </p>
        {rate && <p className="text-qb-ink-muted">Changes apply from now on; posted sales keep how their tax was split.</p>}
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy || chosen.length < 2 || !name.trim()}>
            {rate ? 'Save' : 'Add combined rate'}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

/** The sales tax return for the period: each agency, its rates' taxable sales and tax, and what was paid it. */
export function SalesTaxReturn({ start, end, companyBasis }) {
  const [basis, setBasis] = useState(null);
  const shownBasis = basis ?? companyBasis ?? 'accrual';
  const valid = Boolean(start && end && start <= end);
  const { data, error, reload } = useLedgerQuery(() => (valid ? api.getSalesTaxReturn(start, end, shownBasis) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [start, end, shownBasis, reload]);
  return (
    <section className="qb-panel flex min-h-0 flex-col" aria-label="Sales tax return">
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Sales tax return by agency</h2>
        <span className="text-xs text-qb-ink-muted">Same dates as the liability</span>
        <select className="qb-input ml-auto h-row text-sm" aria-label="Basis" value={shownBasis} onChange={(e) => setBasis(e.target.value)}>
          <option value="accrual">Accrual (when invoiced)</option>
          <option value="cash">Cash (when paid)</option>
        </select>
      </div>
      {error && <Notice kind="error" text={error} />}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Agency / rate</th>
              <th className="qb-grid-head text-right">Taxable sales</th>
              <th className="qb-grid-head text-right">Tax</th>
              <th className="qb-grid-head text-right">Paid</th>
            </tr>
          </thead>
          <tbody>
            {data && data.agencies.length === 0 && (
              <tr><td className="qb-grid-cell text-qb-ink-muted" colSpan={4}>No taxed sales or payments in this period.</td></tr>
            )}
            {(data?.agencies ?? []).map((a) => (
              <AgencyRows key={a.agencyVendorId ?? 'none'} agency={a} />
            ))}
            {data && data.agencies.length > 0 && (
              <tr className="bg-qb-row-header font-semibold">
                <td className="qb-grid-cell" colSpan={2}>All agencies</td>
                <td className="qb-grid-cell num">{formatCents(data.taxCents)}</td>
                <td className="qb-grid-cell num">{formatCents(data.paidCents)}</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function AgencyRows({ agency }) {
  return (
    <>
      <tr className="font-semibold">
        <td className="qb-grid-cell" colSpan={2}>{agency.agencyName}</td>
        <td className="qb-grid-cell num">{formatCents(agency.taxCents)}</td>
        <td className="qb-grid-cell num">{formatCents(agency.paidCents)}</td>
      </tr>
      {agency.lines.map((l) => (
        <tr key={l.taxRateId} className="qb-grid-row">
          <td className="qb-grid-cell pl-6">{l.name} ({ratePercent(l.ratePpm)})</td>
          <td className="qb-grid-cell num">{formatCents(l.taxableSalesCents)}</td>
          <td className="qb-grid-cell num">{formatCents(l.taxCents)}</td>
          <td className="qb-grid-cell" />
        </tr>
      ))}
    </>
  );
}
