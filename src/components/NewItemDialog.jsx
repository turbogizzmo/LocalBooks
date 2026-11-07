import { useState } from 'react';
import { createPortal } from 'react-dom';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { api, errorText } from '../lib/api.js';
import { parseMoneyToCents } from '../lib/money.js';

const incomeOnly = (a) => a.classification === 'INCOME';

/**
 * Adds a product or service from a form's item column ("＋ Add new item…"), so a sale doesn't have
 * to stop for the Products and Services list. Asks for what a sales line needs (name, description,
 * rate, income account, taxable); the rest is on the item's record. `onCreated` gets the new item.
 */
export default function NewItemDialog({ accounts, defaultAccountId = null, onClose, onCreated }) {
  const income = accounts.filter(incomeOnly);
  const startAccount = income.some((a) => a.id === defaultAccountId) ? defaultAccountId : (income[0]?.id ?? null);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [rate, setRate] = useState('');
  const [accountId, setAccountId] = useState(startAccount);
  const [taxable, setTaxable] = useState(true);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const rateCents = rate.trim() === '' ? 0 : parseMoneyToCents(rate);

  const submit = async (e) => {
    e.preventDefault();
    // Rendered in a portal, but React still bubbles the submit to the sales form around the grid.
    e.stopPropagation();
    if (!name.trim()) return setError('Enter a name for the product or service.');
    if (rateCents === null || rateCents < 0) return setError('Enter a valid rate.');
    if (accountId === null) return setError('Choose an income account.');
    setBusy(true);
    try {
      const product = await api.createProduct({ name: name.trim(), description: description.trim() || null, rateCents, incomeAccountId: accountId, isTaxable: taxable });
      onCreated(product);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return createPortal(
    <Dialog title="New product or service" onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="grid grid-cols-2 gap-2">
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Name</span>
            <input className="qb-input" value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Rate</span>
            <input className="qb-input num" value={rate} onChange={(e) => setRate(e.target.value)} inputMode="decimal" data-money aria-invalid={rateCents === null} placeholder="0.00" />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Description on sales forms</span>
            <input className="qb-input" value={description} onChange={(e) => setDescription(e.target.value)} />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Income account</span>
            <AccountSelect accounts={accounts} filter={incomeOnly} value={accountId} onChange={setAccountId} />
          </label>
          <label className="col-span-2 flex items-center gap-1.5 text-sm">
            <input type="checkbox" className="accent-qb-green" checked={taxable} onChange={(e) => setTaxable(e.target.checked)} />
            Taxable: charge sales tax when this is sold
          </label>
        </div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>Add</button>
        </div>
      </form>
    </Dialog>,
    document.body,
  );
}
