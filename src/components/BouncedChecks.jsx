// Banking › Record Bounced Check: a customer's check (or other deposited funds) the bank returned.
// Posting reopens the invoice it paid, takes the amount (and any bank fee) out of the bank, and
// can charge the customer a returned-check fee on a new invoice. See bounced_checks.rs.
import { useMemo, useState } from 'react';
import { useTransactionDate } from '../lib/myPreferences.js';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { kindLabel } from './ProfileFields.jsx';
import { formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import DateBox from './DateBox.jsx';

const keyOf = (c) => `${c.kind}:${c.id}`;
const isExpense = (a) => a.classification === 'EXPENSE';
const isIncome = (a) => a.classification === 'INCOME';

function describe(c) {
  if (c.kind === 'PAYMENT') return `${c.customerName ?? 'Customer'} · invoice #${c.invoiceNumber}${c.reference ? ` · check #${c.reference}` : ''}`;
  return `Deposit · ${c.memo ?? 'other funds'}`;
}

function RecordForm({ accounts, candidates, onPosted, onCancel }) {
  const [picked, setPicked] = useState(candidates[0] ? keyOf(candidates[0]) : null);
  const chosen = candidates.find((c) => keyOf(c) === picked) ?? null;
  const [date, setDate] = useTransactionDate();
  const defaultFeeAccount = useMemo(
    () => accounts.find((a) => a.isActive && isExpense(a) && /bank/i.test(a.name))?.id ?? accounts.find((a) => a.isActive && a.accountType === 'EXPENSE')?.id ?? null,
    [accounts],
  );
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && isIncome(a) && !a.isSystem)?.id ?? null, [accounts]);
  const [bankFee, setBankFee] = useState('');
  const [bankFeeAccount, setBankFeeAccount] = useState(defaultFeeAccount);
  const [customerFee, setCustomerFee] = useState('');
  const [customerFeeAccount, setCustomerFeeAccount] = useState(defaultIncome);
  const [memo, setMemo] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  const bankFeeCents = bankFee.trim() === '' ? 0 : parseMoneyToCents(bankFee);
  const customerFeeCents = customerFee.trim() === '' ? 0 : parseMoneyToCents(customerFee);
  const fromCustomer = chosen?.kind === 'PAYMENT';

  const save = async (e) => {
    e.preventDefault();
    if (!chosen) return setError('Choose the check that bounced.');
    if (bankFeeCents === null || bankFeeCents < 0) return setError('Enter the bank’s fee as an amount, or leave it blank.');
    if (customerFeeCents === null || customerFeeCents < 0) return setError('Enter the customer’s fee as an amount, or leave it blank.');
    setBusy(true);
    try {
      const recorded = await api.recordBouncedCheck({
        kind: chosen.kind,
        id: chosen.id,
        bounceDate: date,
        bankFeeCents,
        bankFeeAccountId: bankFeeCents > 0 ? bankFeeAccount : null,
        customerFeeCents: fromCustomer ? customerFeeCents : 0,
        customerFeeAccountId: fromCustomer && customerFeeCents > 0 ? customerFeeAccount : null,
        memo: memo.trim() || null,
      });
      onPosted(
        `Bounced check of ${formatCents(recorded.amountCents)} recorded: ` +
          (fromCustomer ? `invoice #${recorded.invoiceNumber} is open again, ` : '') +
          `${accountLabel(accounts, recorded.bankAccountId)} is down ${formatCents(recorded.amountCents + recorded.bankFeeCents)}` +
          (recorded.feeInvoiceNumber ? `, and invoice #${recorded.feeInvoiceNumber} charges the returned-check fee.` : '.'),
      );
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <form className="space-y-1 px-2 py-1" onSubmit={save}>
        <div className="flex flex-wrap items-end gap-2">
          <h2 className="mr-2 self-center text-md font-semibold">Record bounced check</h2>
          <label>
            <span className="qb-label">Check that bounced</span>
            <select className="qb-input w-[420px]" value={picked ?? ''} onChange={(e) => setPicked(e.target.value || null)}>
              {candidates.length === 0 && <option value="">No deposited checks to choose from</option>}
              {candidates.map((c) => (
                <option key={keyOf(c)} value={keyOf(c)}>
                  {formatDate(c.date)} · {formatCents(c.amountCents)} · {describe(c)}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span className="qb-label">Date it bounced</span>
            <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          {chosen && (
            <span className="self-center text-sm text-qb-ink-muted">
              Comes out of {accountLabel(accounts, chosen.bankAccountId)}
              {chosen.paymentMethod ? ` · paid by ${chosen.paymentMethodName ?? kindLabel(chosen.paymentMethod).toLowerCase()}` : ''}
            </span>
          )}
        </div>
        <div className="flex flex-wrap items-end gap-2">
          <label>
            <span className="qb-label">Bank fee</span>
            <input className="qb-input num w-[100px]" value={bankFee} onChange={(e) => setBankFee(e.target.value)} placeholder="0.00" inputMode="decimal" data-money />
          </label>
          <label>
            <span className="qb-label">Bank fee account</span>
            <AccountSelect accounts={accounts} filter={isExpense} value={bankFeeAccount} onChange={setBankFeeAccount} className="qb-input w-[210px]" disabled={!bankFeeCents} />
          </label>
          {fromCustomer && (
            <>
              <label>
                <span className="qb-label">Customer fee</span>
                <input className="qb-input num w-[100px]" value={customerFee} onChange={(e) => setCustomerFee(e.target.value)} placeholder="0.00" inputMode="decimal" data-money />
              </label>
              <label>
                <span className="qb-label">Customer fee income account</span>
                <AccountSelect accounts={accounts} filter={(a) => isIncome(a) && !a.isSystem} value={customerFeeAccount} onChange={setCustomerFeeAccount} className="qb-input w-[210px]" disabled={!customerFeeCents} />
              </label>
            </>
          )}
          <label>
            <span className="qb-label">Memo</span>
            <input className="qb-input w-[200px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="e.g. NSF" />
          </label>
          <div className="ml-auto flex items-center gap-2 self-center">
            {error && <span className="text-sm text-qb-negative">{error}</span>}
            <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
              Close
            </button>
            <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !chosen}>
              {busy ? 'Posting…' : 'Record Bounced Check'}
            </button>
          </div>
        </div>
      </form>
    </section>
  );
}

export default function BouncedChecks({ accounts }) {
  const { data, error } = useLedgerQuery(api.listBouncedChecks);
  const candidates = useLedgerQuery(api.listBounceCandidates);
  const [recording, setRecording] = useState(true);
  const [notice, setNotice] = useState(null);
  const rows = data ?? [];

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {recording && candidates.data && (
        <RecordForm
          key={(candidates.data ?? []).map(keyOf).join(',')}
          accounts={accounts}
          candidates={candidates.data}
          onPosted={(text) => setNotice({ kind: 'success', text })}
          onCancel={() => setRecording(false)}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {(error || candidates.error) && <Notice kind="error" text={`Could not load bounced checks: ${error ?? candidates.error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Bounced checks</h2>
          <span className="text-xs text-qb-ink-muted">Only checks that reached the bank can bounce; void a payment still in Undeposited Funds instead</span>
          {!recording && (
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setRecording(true)}>
              Record bounced check
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[92px]" />
              <col />
              <col className="w-[200px]" />
              <col className="w-[100px]" />
              <col className="w-[100px]" />
              <col className="w-[100px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Customer / memo</th>
                <th className="qb-grid-head">Bank</th>
                <th className="qb-grid-head text-right">Amount</th>
                <th className="qb-grid-head text-right">Bank fee</th>
                <th className="qb-grid-head">Fee invoice</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No bounced checks recorded.</td>
                </tr>
              )}
              {rows.map((b) => (
                <tr key={b.id} className={`qb-grid-row ${b.isVoided ? 'text-qb-ink-faint' : ''}`}>
                  <td className="qb-grid-cell">{formatDate(b.bounceDate)}</td>
                  <td className="qb-grid-cell">
                    {b.customerName ? (
                      <>
                        <span className="font-semibold">{b.customerName}</span>
                        <span className="text-qb-ink-muted">
                          {' '}
                          · invoice #{b.invoiceNumber}
                          {b.reference ? ` · check #${b.reference}` : ''}
                        </span>
                      </>
                    ) : (
                      'Deposit'
                    )}
                    {b.memo ? <span className="text-qb-ink-muted"> · {b.memo}</span> : null}
                    {b.isVoided && <span className="ml-1 text-2xs font-semibold">VOIDED</span>}
                  </td>
                  <td className="qb-grid-cell">{accountLabel(accounts, b.bankAccountId)}</td>
                  <td className={`qb-grid-cell num ${b.isVoided ? 'line-through' : ''}`}>{formatCents(b.amountCents)}</td>
                  <td className="qb-grid-cell num">{b.bankFeeCents ? formatCents(b.bankFeeCents) : ''}</td>
                  <td className="qb-grid-cell">{b.feeInvoiceNumber ? `#${b.feeInvoiceNumber}` : ''}</td>
                  <td className="qb-grid-cell cell-tight text-center">
                    <RowMenu
                      label={`Actions for the bounced check of ${formatCents(b.amountCents)}`}
                      items={[
                        {
                          label: b.isVoided ? 'Voided' : 'Void bounced check',
                          danger: true,
                          disabled: b.isVoided,
                          confirm: {
                            title: 'Void this bounced check?',
                            body: `The ${formatCents(b.amountCents)} goes back into the bank${b.bankFeeCents ? ' with the bank fee' : ''}, the payment counts toward its invoice again${b.feeInvoiceNumber ? `, and fee invoice #${b.feeInvoiceNumber} is voided` : ''}. A reversing entry keeps the history.`,
                            confirmLabel: 'Void',
                          },
                          onSelect: async () => {
                            await api.voidBouncedCheck(b.id);
                            setNotice({ kind: 'success', text: 'Bounced check voided: the payment counts again.' });
                          },
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
    </div>
  );
}
