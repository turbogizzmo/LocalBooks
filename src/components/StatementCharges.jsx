// Customers › Enter Statement Charges: a customer's own register of charges billed on their next
// statement instead of on an invoice (statement_charges.rs). Each posts to Accounts Receivable for
// the customer, ages like an invoice, and takes payments in Receive Payments.
import { useCallback, useMemo, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import DateBox from './DateBox.jsx';
import Notice from './Notice.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents, parseQuantityE4, quantityAmount } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

const incomeOnly = (a) => a.classification === 'INCOME';

/** The register and the row to enter a new charge. `customer` is { id, name }. */
export default function StatementCharges({ customer, onClose }) {
  const accounts = useLedgerQuery(api.listAccounts).data ?? [];
  const charges = useLedgerQuery(useCallback(() => api.listStatementCharges(customer.id), [customer.id]));
  const products = useLedgerQuery(api.listProducts).data ?? [];
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && incomeOnly(a))?.id ?? null, [accounts]);
  const blank = () => ({ date: todayIso(), dueDate: '', productId: null, description: '', qty: '1', rate: '', accountId: defaultIncome });
  const [entry, setEntry] = useState(blank);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);
  const set = (key) => (value) => setEntry((e) => ({ ...e, [key]: value }));

  const qtyE4 = parseQuantityE4(entry.qty);
  const rateCents = parseMoneyToCents(entry.rate);
  const amount = qtyE4 !== null && rateCents !== null && rateCents > 0 ? quantityAmount(qtyE4, rateCents) : null;
  const ready = Boolean(entry.date && entry.description.trim() && entry.accountId && amount);

  const pickProduct = (id) => {
    const product = products.find((p) => p.id === id);
    setEntry((e) => ({
      ...e,
      productId: id,
      description: product ? (product.description ?? product.name) : e.description,
      rate: product?.rateCents ? centsToInput(product.rateCents) : e.rate,
      accountId: product?.incomeAccountId ?? e.accountId,
    }));
  };

  const record = async (ev) => {
    ev.preventDefault();
    if (!ready) return;
    setBusy(true);
    setNotice(null);
    try {
      await api.createStatementCharge({
        customerName: customer.name,
        chargeDate: entry.date,
        dueDate: entry.dueDate || null,
        productServiceId: entry.productId,
        description: entry.description.trim(),
        quantityE4: qtyE4,
        rateCents,
        incomeAccountId: entry.accountId,
      });
      setEntry(blank());
      setNotice({ kind: 'success', text: `Charge of ${formatCents(amount)} recorded. It's billed on ${customer.name}'s next statement.` });
      await charges.reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const rows = charges.data ?? [];
  const open = rows.filter((c) => !c.isVoided).reduce((sum, c) => sum + c.balanceCents, 0);
  return (
    <Dialog title={`Statement charges · ${customer.name}`} onClose={onClose}>
      <form onSubmit={record} className="flex min-h-0 flex-col gap-2 p-3">
        <p className="text-sm text-qb-ink-muted">
          Charges entered here go on {customer.name}&apos;s next statement instead of an invoice. Each one posts to Accounts Receivable, shows in the A/R aging, and is paid in Receive Payments.
        </p>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {charges.error && <Notice kind="error" text={charges.error} />}
        <div className="min-h-0 max-h-[260px] overflow-auto rounded-xs border border-qb-border scrollbar-thin">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head w-[92px]">Date</th>
                <th className="qb-grid-head">Description</th>
                <th className="qb-grid-head w-[92px]">Due</th>
                <th className="qb-grid-head w-[96px] text-right">Amount</th>
                <th className="qb-grid-head w-[96px] text-right">Open</th>
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 && (
                <tr>
                  <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">No statement charges yet.</td>
                </tr>
              )}
              {rows.map((c) => (
                <tr key={c.id} className={`qb-grid-row ${c.isVoided ? 'text-qb-ink-faint line-through' : ''}`}>
                  <td className="qb-grid-cell">{formatDate(c.issueDate)}</td>
                  <td className="qb-grid-cell" title={c.description}>{c.description}</td>
                  <td className="qb-grid-cell">{formatDate(c.dueDate)}</td>
                  <td className="qb-grid-cell num">{formatCents(c.totalCents)}</td>
                  <td className="qb-grid-cell num">{c.isVoided ? 'Void' : c.balanceCents ? formatCents(c.balanceCents) : 'Paid'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="text-right text-sm">
          Open charges <span className="num font-semibold">{formatCents(open)}</span>
        </div>
        <fieldset className="grid grid-cols-[132px_1fr] items-end gap-2 rounded-xs border border-qb-border p-2" disabled={busy}>
          <legend className="px-1 text-xs font-semibold text-qb-ink-muted">New charge</legend>
          <label>
            <span className="qb-label">Date</span>
            <DateBox className="qb-input w-full" value={entry.date} onChange={(e) => set('date')(e.target.value)} />
          </label>
          <label>
            <span className="qb-label">Item</span>
            <select className="qb-input w-full" value={entry.productId ?? ''} onChange={(e) => pickProduct(e.target.value === '' ? null : Number(e.target.value))}>
              <option value="">No item</option>
              {products
                .filter((p) => p.isActive && (!p.itemType || p.itemType === 'ITEM'))
                .map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
            </select>
          </label>
          <label className="col-span-2">
            <span className="qb-label">Description</span>
            <input className="qb-input w-full" value={entry.description} onChange={(e) => set('description')(e.target.value)} placeholder="What the charge is for" />
          </label>
          <label>
            <span className="qb-label">Due date</span>
            <DateBox className="qb-input w-full" value={entry.dueDate} onChange={(e) => set('dueDate')(e.target.value)} title="Leave blank to make it due on the charge date" />
          </label>
          <div className="flex items-end gap-2">
            <label>
              <span className="qb-label">Qty</span>
              <input className="qb-input num w-[70px]" value={entry.qty} onChange={(e) => set('qty')(e.target.value)} inputMode="decimal" data-quantity aria-invalid={qtyE4 === null} />
            </label>
            <label>
              <span className="qb-label">Rate</span>
              <input className="qb-input num w-[96px]" value={entry.rate} onChange={(e) => set('rate')(e.target.value)} inputMode="decimal" data-money placeholder="0.00" />
            </label>
            <label className="min-w-0 flex-1">
              <span className="qb-label">Income account</span>
              <AccountSelect accounts={accounts} filter={incomeOnly} value={entry.accountId} onChange={set('accountId')} className="qb-input w-full" />
            </label>
          </div>
        </fieldset>
        <div className="flex items-center justify-end gap-2">
          <span className="mr-auto text-sm">
            Amount <span className="num font-semibold">{amount ? formatCents(amount) : '—'}</span>
          </span>
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>
            Close
          </button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={!ready || busy}>
            Record charge
          </button>
        </div>
      </form>
    </Dialog>
  );
}
