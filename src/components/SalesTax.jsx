import { useEffect, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, mulDivRound, parseMoneyToCents } from '../lib/money.js';
import { formatDate, periodRange, todayIso } from '../lib/dates.js';

/** Sales tax as set up for the company: on or off, and the rates. */
export function useSalesTax() {
  const prefs = useLedgerQuery(api.getPreferences).data;
  const rates = useLedgerQuery(api.listTaxRates).data ?? [];
  return { enabled: prefs?.sales_tax_enabled === 'true', rates, activeRates: rates.filter((r) => r.isActive) };
}

/** "8.25%" from parts per million. */
export const ratePercent = (ppm) => `${(ppm / 10_000).toFixed(ppm % 10_000 === 0 ? 0 : ppm % 1_000 === 0 ? 1 : ppm % 100 === 0 ? 2 : 4)}%`;

/** Parses "8.25" or "8.25%" to parts per million. */
export function parsePercentToPpm(text) {
  const t = String(text).trim().replace(/%$/, '');
  if (!/^\d{1,3}(\.\d{0,4})?$/.test(t)) return null;
  const [whole, frac = ''] = t.split('.');
  const ppm = Number(whole) * 10_000 + Number(frac.padEnd(4, '0'));
  return ppm <= 1_000_000 ? ppm : null;
}

/** The tax on a document's lines at `rate`, rounded exactly as the ledger rounds it. */
export function taxOn(lines, rate, amountOf = (l) => l.amount) {
  if (!rate) return 0;
  const taxable = lines.filter((l) => l.isTaxable).reduce((s, l) => s + (amountOf(l) ?? 0), 0);
  return mulDivRound(taxable, rate.ratePpm, 1_000_000) ?? 0;
}

/** Tax rate dropdown for a sales form ("No tax" plus the active rates). */
export function TaxRatePicker({ rates, value, onChange, className = 'qb-input w-[170px]' }) {
  return (
    <label>
      <span className="qb-label">Sales tax</span>
      <select className={className} value={value ?? ''} onChange={(e) => onChange(e.target.value === '' ? null : Number(e.target.value))}>
        <option value="">No tax</option>
        {rates.filter((r) => r.isActive || r.id === value).map((r) => (
          <option key={r.id} value={r.id}>
            {r.name} ({ratePercent(r.ratePpm)})
          </option>
        ))}
      </select>
    </label>
  );
}

/**
 * Tax for one sales form: the chosen rate (defaulting from the customer: their rate, or none when
 * exempt) and the amount on the form's taxable lines.
 */
export function useFormTax(customerName, customers) {
  const tax = useSalesTax();
  const [rateId, setRateId] = useState(null);
  const [touched, setTouched] = useState(false);
  const customer = (customers ?? []).find((c) => c.name.toLowerCase() === customerName.trim().toLowerCase());
  useEffect(() => {
    if (touched || !tax.enabled) return;
    // The customer's own rate; otherwise the company's only rate, if it has just one.
    const only = tax.activeRates.length === 1 ? tax.activeRates[0].id : null;
    if (customer) setRateId(customer.isTaxExempt ? null : (customer.taxRateId ?? only));
    else if (rateId === null) setRateId(only);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [customer?.id, tax.enabled, tax.activeRates.length]);
  const rate = tax.rates.find((r) => r.id === rateId) ?? null;
  return {
    enabled: tax.enabled,
    rateId: tax.enabled ? rateId : null,
    rate: tax.enabled ? rate : null,
    exempt: Boolean(customer?.isTaxExempt),
    picker: tax.enabled ? (
      <TaxRatePicker
        rates={tax.rates}
        value={rateId}
        onChange={(id) => {
          setTouched(true);
          setRateId(id);
        }}
      />
    ) : null,
    /** Sets the rate from a document being loaded (an estimate becoming an invoice). */
    set: (id) => {
      setTouched(true);
      setRateId(id);
    },
    reset: () => {
      setTouched(false);
      setRateId(tax.activeRates.length === 1 ? tax.activeRates[0].id : null);
    },
  };
}

function RateDialog({ rate, accounts, onClose, onSaved }) {
  const [name, setName] = useState(rate?.name ?? '');
  const [percent, setPercent] = useState(rate ? (rate.ratePpm / 10_000).toString() : '');
  const [agency, setAgency] = useState(rate?.agencyName ?? '');
  const [accountId, setAccountId] = useState(rate?.liabilityAccountId ?? null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const ppm = parsePercentToPpm(percent);

  const submit = async (e) => {
    e.preventDefault();
    if (ppm === null) return setError('Enter the rate as a percentage, such as 8.25.');
    setBusy(true);
    try {
      const input = { name, ratePpm: ppm, agencyName: agency.trim() || null, liabilityAccountId: accountId };
      const saved = rate ? await api.updateTaxRate(rate.id, input) : await api.createTaxRate(input);
      onSaved(`${rate ? 'Saved' : 'Added'} ${saved.name} at ${ratePercent(saved.ratePpm)}.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Dialog title={rate ? `Edit ${rate.name}` : 'New sales tax rate'} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="grid grid-cols-2 gap-2">
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Name</span>
            <input className="qb-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. State + County" />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Rate (%)</span>
            <input className="qb-input num" value={percent} onChange={(e) => setPercent(e.target.value)} placeholder="8.25" inputMode="decimal" aria-invalid={percent !== '' && ppm === null} />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Agency (who you pay)</span>
            <input className="qb-input" value={agency} onChange={(e) => setAgency(e.target.value)} placeholder="e.g. State Department of Revenue" />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Liability account</span>
            <AccountSelect accounts={accounts} filter={(a) => a.accountType === 'OTHER_CURRENT_LIABILITY'} value={accountId} onChange={setAccountId} placeholder="Sales Tax Payable (default)" />
          </label>
        </div>
        {rate && <p className="text-qb-ink-muted">A new rate applies to transactions from now on; posted transactions keep the tax they were posted with.</p>}
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>{rate ? 'Save' : 'Add rate'}</button>
        </div>
      </form>
    </Dialog>
  );
}

function PayDialog({ rates, accounts, owedCents, onClose, onPaid }) {
  const [rateId, setRateId] = useState(rates.length === 1 ? rates[0].id : null);
  const [accountId, setAccountId] = useState(() => accounts.find((a) => a.isActive && a.accountType === 'BANK')?.id ?? null);
  const [date, setDate] = useState(todayIso());
  const [amount, setAmount] = useState(owedCents > 0 ? centsToInput(owedCents) : '');
  const [reference, setReference] = useState('');
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const cents = parseMoneyToCents(amount);

  const submit = async (e) => {
    e.preventDefault();
    if (accountId === null) return setError('Choose the account it is paid from.');
    if (cents === null || cents <= 0) return setError('Enter the amount paid.');
    setBusy(true);
    try {
      await api.paySalesTax({ taxRateId: rateId, accountId, paymentDate: date, amountCents: cents, reference: reference.trim() || null });
      onPaid(`Recorded a ${formatCents(cents)} sales tax payment: Dr Sales Tax Payable / Cr the payment account.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Dialog title="Pay sales tax" onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <p className="text-qb-ink-muted">Owed now: {formatCents(owedCents)}.</p>
        <div className="grid grid-cols-2 gap-2">
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">For</span>
            <select className="qb-input" value={rateId ?? ''} onChange={(e) => setRateId(e.target.value === '' ? null : Number(e.target.value))}>
              <option value="">All rates (Sales Tax Payable)</option>
              {rates.map((r) => (
                <option key={r.id} value={r.id}>{r.name}{r.agencyName ? ` · ${r.agencyName}` : ''}</option>
              ))}
            </select>
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Paid from</span>
            <AccountSelect accounts={accounts} filter={(a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD'} value={accountId} onChange={setAccountId} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Payment date</span>
            <input type="date" className="qb-input" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Amount</span>
            <input className="qb-input num" value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Reference (confirmation or check no.)</span>
            <input className="qb-input" value={reference} onChange={(e) => setReference(e.target.value)} />
          </label>
        </div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>Record payment</button>
        </div>
      </form>
    </Dialog>
  );
}

/** Sales › Sales Tax: turn it on, set up rates, see what is owed and record payments. */
export default function SalesTax({ accounts }) {
  const prefs = useLedgerQuery(api.getPreferences).data;
  const { data: rates, error } = useLedgerQuery(api.listTaxRates);
  const payments = useLedgerQuery(api.listSalesTaxPayments);
  const [[start, end], setRange] = useState(() => periodRange('thisQuarter'));
  const liability = useLedgerQuery(() => api.getTaxLiability(start, end));
  const [dialog, setDialog] = useState(null);
  const [notice, setNotice] = useState(null);
  const enabled = prefs?.sales_tax_enabled === 'true';

  useEffect(() => {
    if (start && end && start <= end) liability.reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [start, end]);

  const toggle = async (on) => {
    try {
      await api.setPreference('sales_tax_enabled', on ? 'true' : 'false');
      setNotice({ kind: 'success', text: on ? 'Sales tax is on: sales forms show a Tax column and a tax rate.' : 'Sales tax is off. Posted tax is unchanged.' });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const done = (text) => {
    setDialog(null);
    setNotice({ kind: 'success', text });
  };
  const report = liability.data;

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {dialog?.mode === 'rate' && <RateDialog rate={dialog.rate} accounts={accounts} onClose={() => setDialog(null)} onSaved={done} />}
      {dialog?.mode === 'pay' && <PayDialog rates={(rates ?? []).filter((r) => r.isActive)} accounts={accounts} owedCents={report?.owedCents ?? 0} onClose={() => setDialog(null)} onPaid={done} />}
      {prefs && (
        <div className={`qb-panel flex flex-none items-center gap-2 px-2 py-1 text-sm ${enabled ? 'bg-qb-green-tint' : 'bg-[#fff4e5]'}`}>
          <label className="flex items-center gap-1.5 font-semibold">
            <input type="checkbox" className="accent-qb-green" checked={enabled} onChange={(e) => toggle(e.target.checked)} />
            Charge sales tax
          </label>
          <span className="text-xs text-qb-ink-muted">
            {enabled
              ? 'Invoices, sales receipts, estimates and credit memos have a Tax column and a rate; mark products taxable or not, and customers exempt, on their records.'
              : 'Turn this on to charge sales tax on sales forms.'}
          </span>
        </div>
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-2">
        <section className="qb-panel flex min-h-0 flex-col">
          <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
            <h2 className="text-md font-semibold">Tax rates</h2>
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setDialog({ mode: 'rate', rate: null })}>
              New rate
            </button>
          </div>
          <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head">Name</th>
                  <th className="qb-grid-head text-right">Rate</th>
                  <th className="qb-grid-head">Agency</th>
                  <th className="qb-grid-head">Status</th>
                  <th className="qb-grid-head w-[30px]" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {rates && rates.length === 0 && (
                  <tr>
                    <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">No rates yet. Add the combined rate you charge, such as 8.25%.</td>
                  </tr>
                )}
                {(rates ?? []).map((r) => (
                  <tr key={r.id} className={`qb-grid-row ${r.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setDialog({ mode: 'rate', rate: r })}>
                    <td className="qb-grid-cell font-semibold">{r.name}</td>
                    <td className="qb-grid-cell num">{ratePercent(r.ratePpm)}</td>
                    <td className="qb-grid-cell">{r.agencyName ?? ''}</td>
                    <td className="qb-grid-cell text-sm">{r.isActive ? 'Active' : 'Inactive'}</td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        label={`Actions for ${r.name}`}
                        items={[
                          { label: 'Edit…', onSelect: () => setDialog({ mode: 'rate', rate: r }) },
                          {
                            label: r.isActive ? 'Make inactive' : 'Make active',
                            onSelect: () => api.setTaxRateActive(r.id, !r.isActive).then(() => setNotice({ kind: 'success', text: `${r.name} is ${r.isActive ? 'inactive' : 'active'}.` }), (err) => setNotice({ kind: 'error', text: errorText(err) })),
                          },
                        ]}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
        <section className="qb-panel flex min-h-0 flex-col">
          <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
            <h2 className="text-md font-semibold">Sales tax liability</h2>
            <input type="date" className="qb-input h-row w-[128px] text-sm" value={start} onChange={(e) => setRange([e.target.value, end])} aria-label="From" />
            <span className="text-sm text-qb-ink-muted">to</span>
            <input type="date" className="qb-input h-row w-[128px] text-sm" value={end} onChange={(e) => setRange([start, e.target.value])} aria-label="To" />
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setDialog({ mode: 'pay' })}>
              Pay sales tax
            </button>
          </div>
          {liability.error && <Notice kind="error" text={liability.error} />}
          <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head">Rate</th>
                  <th className="qb-grid-head text-right">Taxable sales</th>
                  <th className="qb-grid-head text-right">Non-taxable</th>
                  <th className="qb-grid-head text-right">Tax</th>
                </tr>
              </thead>
              <tbody>
                {report && report.rows.length === 0 && (
                  <tr>
                    <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">No sales in this period.</td>
                  </tr>
                )}
                {(report?.rows ?? []).map((r) => (
                  <tr key={r.taxRateId ?? 'none'} className="qb-grid-row">
                    <td className="qb-grid-cell">{r.name}{r.ratePpm != null ? ` (${ratePercent(r.ratePpm)})` : ''}</td>
                    <td className="qb-grid-cell num">{formatCents(r.taxableSalesCents)}</td>
                    <td className="qb-grid-cell num">{formatCents(r.nonTaxableSalesCents)}</td>
                    <td className="qb-grid-cell num font-semibold">{formatCents(r.taxCollectedCents)}</td>
                  </tr>
                ))}
                {report && (
                  <>
                    <tr className="bg-qb-row-header font-semibold">
                      <td className="qb-grid-cell" colSpan={3}>Tax for the period</td>
                      <td className="qb-grid-cell num">{formatCents(report.taxCollectedCents)}</td>
                    </tr>
                    <tr className="font-semibold">
                      <td className="qb-grid-cell" colSpan={3}>Paid in the period</td>
                      <td className="qb-grid-cell num">{formatCents(report.paidCents)}</td>
                    </tr>
                    <tr className="font-semibold">
                      <td className="qb-grid-cell" colSpan={3}>Owed as of {formatDate(report.endDate)}</td>
                      <td className={`qb-grid-cell num ${report.owedCents > 0 ? 'text-qb-negative' : ''}`}>{formatCents(report.owedCents)}</td>
                    </tr>
                  </>
                )}
              </tbody>
            </table>
            <div className="border-t border-qb-border px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Payments</div>
            <table className="qb-grid">
              <tbody>
                {(payments.data ?? []).length === 0 && (
                  <tr>
                    <td className="qb-grid-cell text-qb-ink-muted">No sales tax payments yet.</td>
                  </tr>
                )}
                {(payments.data ?? []).map((p) => (
                  <tr key={p.id} className={`qb-grid-row ${p.isVoided ? 'text-qb-ink-faint' : ''}`}>
                    <td className="qb-grid-cell">{formatDate(p.paymentDate)}</td>
                    <td className="qb-grid-cell">{p.taxRateName ?? 'All rates'}</td>
                    <td className="qb-grid-cell">{p.accountName}{p.reference ? ` · ${p.reference}` : ''}</td>
                    <td className="qb-grid-cell num">{formatCents(p.amountCents)}</td>
                    <td className="qb-grid-cell cell-tight w-[30px] text-center">
                      <RowMenu
                        items={[
                          voidItem({
                            journalEntryId: p.journalEntryId,
                            voided: p.isVoided,
                            description: `Sales tax payment of ${formatCents(p.amountCents)}.`,
                            onVoid: (id) => api.voidTransaction(id).then(() => setNotice({ kind: 'success', text: 'Voided the sales tax payment.' })),
                          }),
                        ]}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      </div>
    </div>
  );
}
