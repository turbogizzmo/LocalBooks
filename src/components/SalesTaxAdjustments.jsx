import { useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { Dialog } from './InventoryDialog.jsx';
import DateBox from './DateBox.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

/**
 * Adjust Sales Tax (gaps row T70): raises or lowers what is owed on one rate, against an income or
 * expense account, for a timely-filing discount, a penalty, rounding on the return and the like.
 * It shows on the sales tax return under the rate's agency.
 */
export function AdjustDialog({ rates, accounts, onClose, onSaved }) {
  // Combined rates are adjusted part by part, so each agency's return is right.
  const usable = rates.filter((r) => r.isActive && !r.isGroup);
  const [rateId, setRateId] = useState(usable.length === 1 ? usable[0].id : null);
  const [date, setDate] = useState(todayIso());
  const [direction, setDirection] = useState('reduce');
  const [amount, setAmount] = useState('');
  const [accountId, setAccountId] = useState(null);
  const [memo, setMemo] = useState('');
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const cents = parseMoneyToCents(amount);

  const submit = async (e) => {
    e.preventDefault();
    if (rateId === null) return setError('Choose the rate to adjust.');
    if (cents === null || cents <= 0) return setError('Enter the amount.');
    if (accountId === null) return setError(direction === 'reduce' ? 'Choose the income account the reduction goes to.' : 'Choose the expense account the increase comes from.');
    setBusy(true);
    try {
      const signed = direction === 'reduce' ? -cents : cents;
      await api.adjustSalesTax({ taxRateId: rateId, adjustmentDate: date, amountCents: signed, accountId, memo: memo.trim() || null });
      const name = usable.find((r) => r.id === rateId)?.name ?? 'the rate';
      onSaved(`${direction === 'reduce' ? 'Lowered' : 'Raised'} the tax owed on ${name} by ${formatCents(cents)}.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Dialog title="Adjust sales tax" onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="grid grid-cols-2 gap-2">
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Rate</span>
            <select className="qb-input" value={rateId ?? ''} onChange={(e) => setRateId(e.target.value === '' ? null : Number(e.target.value))}>
              <option value="">Choose a rate…</option>
              {usable.map((r) => (
                <option key={r.id} value={r.id}>{r.name}{r.agencyName ? ` · ${r.agencyName}` : ''}</option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Adjustment date</span>
            <DateBox className="qb-input" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Amount</span>
            <input className="qb-input num" value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" />
          </label>
          <fieldset className="col-span-2 flex gap-4">
            <legend className="sr-only">Direction</legend>
            <label className="flex items-center gap-1.5">
              <input type="radio" name="direction" className="accent-qb-green" checked={direction === 'reduce'} onChange={() => setDirection('reduce')} />
              Reduce sales tax owed
            </label>
            <label className="flex items-center gap-1.5">
              <input type="radio" name="direction" className="accent-qb-green" checked={direction === 'increase'} onChange={() => setDirection('increase')} />
              Increase sales tax owed
            </label>
          </fieldset>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">{direction === 'reduce' ? 'Income account (e.g. a timely-filing discount)' : 'Expense account (e.g. penalties)'}</span>
            <AccountSelect accounts={accounts} filter={(a) => a.classification === 'INCOME' || a.classification === 'EXPENSE'} value={accountId} onChange={setAccountId} />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Memo</span>
            <input className="qb-input" value={memo} onChange={(e) => setMemo(e.target.value)} />
          </label>
        </div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>Record adjustment</button>
        </div>
      </form>
    </Dialog>
  );
}

/** The adjustments recorded, newest first, each with Void. */
export function AdjustmentList({ onNotice }) {
  const adjustments = useLedgerQuery(api.listSalesTaxAdjustments);
  const rows = adjustments.data ?? [];
  if (rows.length === 0) return null;
  return (
    <>
      <div className="border-t border-qb-border px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Adjustments</div>
      <table className="qb-grid">
        <tbody>
          {rows.map((a) => (
            <tr key={a.id} className={`qb-grid-row ${a.isVoided ? 'text-qb-ink-faint' : ''}`}>
              <td className="qb-grid-cell">{formatDate(a.adjustmentDate)}</td>
              <td className="qb-grid-cell">{a.taxRateName}</td>
              <td className="qb-grid-cell">{a.accountName}{a.memo ? ` · ${a.memo}` : ''}</td>
              <td className="qb-grid-cell num">{formatCents(a.amountCents)}</td>
              <td className="qb-grid-cell cell-tight w-[30px] text-center">
                <RowMenu
                  items={[
                    voidItem({
                      journalEntryId: a.journalEntryId,
                      voided: a.isVoided,
                      description: `Sales tax adjustment of ${formatCents(a.amountCents)} on ${a.taxRateName}.`,
                      onVoid: (id) => api.voidTransaction(id).then(() => onNotice({ kind: 'success', text: 'Voided the sales tax adjustment.' })),
                    }),
                  ]}
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  );
}
