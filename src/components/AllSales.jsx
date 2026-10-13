import { useEffect, useMemo, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import LineItemGrid, { blankLine, summarizeLines } from './LineItemGrid.jsx';
import MoneyBar from './MoneyBar.jsx';
import Notice from './Notice.jsx';
import { depositFilter, StatusBadge } from './Invoices.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { useInvoicePrinter } from './PrintDocuments.jsx';
import { api, errorText, PAYMENT_METHODS, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';

const KIND_LABEL = { INVOICE: 'Invoice', PAYMENT: 'Payment', SALES_RECEIPT: 'Sales Receipt' };

function SalesReceiptForm({ accounts, onPosted, onCancel }) {
  const products = useLedgerQuery(api.listProducts);
  const customers = useLedgerQuery(api.listCustomers);
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && a.classification === 'INCOME')?.id ?? null, [accounts]);
  const defaultDeposit = useMemo(() => accounts.find((a) => a.isActive && a.accountType === 'BANK')?.id ?? null, [accounts]);
  const [customer, setCustomer] = useState('');
  const [date, setDate] = useState(todayIso());
  const [method, setMethod] = useState('CASH');
  const [reference, setReference] = useState('');
  const [depositId, setDepositId] = useState(defaultDeposit);
  const [memo, setMemo] = useState('');
  const [lines, setLines] = useState(() => Array.from({ length: 3 }, () => blankLine(defaultIncome)));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    if (depositId === null && defaultDeposit !== null) setDepositId(defaultDeposit);
  }, [defaultDeposit, depositId]);
  useEffect(() => {
    if (defaultIncome === null) return;
    setLines((ls) => ls.map((l) => (l.accountId === null ? { ...l, accountId: defaultIncome } : l)));
  }, [defaultIncome]);

  const { filled, invalidCount, total, payload } = summarizeLines(lines);

  const save = async () => {
    if (filled.length === 0) return setError('Add at least one line.');
    if (invalidCount > 0) return setError(`${invalidCount} line(s) need an income account, a whole-number quantity and a rate above zero.`);
    if (depositId === null) return setError('Choose where the money was deposited.');
    setBusy(true);
    try {
      const receipt = await api.postSalesReceipt({
        customerName: customer.trim() || null,
        receiptDate: date,
        depositAccountId: depositId,
        paymentMethod: method,
        reference: reference || null,
        memo: memo || null,
        lines: payload,
      });
      onPosted(`Sales receipt #${receipt.receiptNumber} posted: Dr ${formatCents(receipt.totalCents)} to the deposit account / Cr income.`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">New sales receipt</h2>
        <label className="w-[220px]">
          <span className="qb-label">Customer (optional)</span>
          <input autoFocus className="qb-input w-full" value={customer} onChange={(e) => setCustomer(e.target.value)} list="receipt-customers" placeholder="Walk-in" />
          <datalist id="receipt-customers">
            {(customers.data ?? []).map((c) => (
              <option key={c.id} value={c.name} />
            ))}
          </datalist>
        </label>
        <label>
          <span className="qb-label">Sales receipt date</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Payment method</span>
          <select className="qb-input w-[140px]" value={method} onChange={(e) => setMethod(e.target.value)}>
            {PAYMENT_METHODS.map((m) => (
              <option key={m.id} value={m.id}>{m.label}</option>
            ))}
          </select>
        </label>
        <label>
          <span className="qb-label">Reference no.</span>
          <input className="qb-input w-[96px]" value={reference} onChange={(e) => setReference(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Deposit to</span>
          <AccountSelect accounts={accounts} filter={depositFilter} value={depositId} onChange={setDepositId} className="qb-input w-[190px]" />
        </label>
        <div className="ml-auto text-right">
          <div className="qb-label">Amount received</div>
          <div className="num text-3xl font-semibold leading-none">{formatCents(total)}</div>
        </div>
      </div>
      <LineItemGrid lines={lines} setLines={setLines} accounts={accounts} products={products.data ?? []} defaultAccountId={defaultIncome} />
      <div className="flex items-center gap-2 px-2 py-1">
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setLines((ls) => [...ls, blankLine(defaultIncome)])}>
          Add line
        </button>
        <input className="qb-input w-[280px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Memo" />
        <div className="ml-auto flex items-center gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
            {busy ? 'Posting…' : 'Save & Post to Ledger'}
          </button>
        </div>
      </div>
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
    </section>
  );
}

export default function AllSales({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listAllSales);
  const [filter, setFilter] = useState('ALL');
  const [composing, setComposing] = useState(intent?.action === 'sales-receipt');

  useEffect(() => {
    if (intent?.action === 'sales-receipt') setComposing(true);
  }, [intent]);
  const [formKey, setFormKey] = useState(0);
  const [notice, setNotice] = useState(null);
  const today = todayIso();
  const since = addDays(today, -30);

  const rows = data ?? [];
  const live = rows.filter((r) => !r.isVoided);
  const openInvoices = live.filter((r) => r.kind === 'INVOICE' && r.balanceCents > 0);
  const overdue = openInvoices.filter((r) => r.dueDate < today);
  const paidRecent = live.filter((r) => (r.kind === 'PAYMENT' || r.kind === 'SALES_RECEIPT') && r.date >= since);
  const [printPortal, printInvoice] = useInvoicePrinter((text) => setNotice({ kind: 'error', text }));
  const voidRow = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    setNotice({ kind: 'success', text: 'Transaction voided: a reversing journal entry now offsets it.' });
  };
  const sum = (list, key) => list.reduce((s, r) => s + r[key], 0);
  const visible = filter === 'ALL' ? rows : rows.filter((r) => r.kind === filter);

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printPortal}
      <MoneyBar
        items={[
          { label: 'Open invoices', icon: '○', color: '#0077c5', cents: sum(openInvoices, 'balanceCents'), count: openInvoices.length },
          { label: 'Overdue', icon: '⚠', color: '#d03b3b', cents: sum(overdue, 'balanceCents'), count: overdue.length },
          { label: 'Paid last 30 days', icon: '✓', color: '#0ca30c', cents: sum(paidRecent, 'totalCents'), count: paidRecent.length },
        ]}
      />

      {composing ? (
        <SalesReceiptForm
          key={formKey}
          accounts={accounts}
          onCancel={() => setComposing(false)}
          onPosted={(text) => {
            setNotice({ kind: 'success', text });
            setFormKey((k) => k + 1);
          }}
        />
      ) : null}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load sales: ${error}`} />}

      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">All sales</h2>
          <select className="qb-input h-row w-[150px] text-sm" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Type">
            <option value="ALL">All transactions</option>
            <option value="INVOICE">Invoices</option>
            <option value="PAYMENT">Payments</option>
            <option value="SALES_RECEIPT">Sales receipts</option>
          </select>
          <span className="text-xs text-qb-ink-muted">{visible.length} transactions</span>
          {!composing && (
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setComposing(true)}>
              New sales receipt
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[92px]" />
              <col className="w-[110px]" />
              <col className="w-[64px]" />
              <col />
              <col className="w-[92px]" />
              <col className="w-[112px]" />
              <col className="w-[112px]" />
              <col className="w-[96px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Type</th>
                <th className="qb-grid-head">No.</th>
                <th className="qb-grid-head">Customer</th>
                <th className="qb-grid-head">Due date</th>
                <th className="qb-grid-head text-right">Balance</th>
                <th className="qb-grid-head text-right">Total</th>
                <th className="qb-grid-head">Status</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && visible.length === 0 && (
                <tr>
                  <td colSpan={9} className="qb-grid-cell text-qb-ink-muted">No sales transactions yet.</td>
                </tr>
              )}
              {visible.map((r) => {
                const isOverdue = !r.isVoided && r.kind === 'INVOICE' && r.balanceCents > 0 && r.dueDate < today;
                const what = `${KIND_LABEL[r.kind]} #${r.number}${r.customerName ? ` for ${r.customerName}` : ''} (${formatCents(r.totalCents)}).`;
                return (
                  <tr key={`${r.kind}-${r.id}`} className={`qb-grid-row ${r.isVoided ? 'text-qb-ink-faint' : ''}`}>
                    <td className="qb-grid-cell">{formatDate(r.date)}</td>
                    <td className="qb-grid-cell">{KIND_LABEL[r.kind]}</td>
                    <td className="qb-grid-cell">{r.number}</td>
                    <td className="qb-grid-cell" title={r.customerName}>{r.customerName || <span className="text-qb-ink-faint">—</span>}</td>
                    <td className={`qb-grid-cell ${isOverdue ? 'font-semibold text-qb-negative' : ''}`}>{formatDate(r.dueDate)}</td>
                    <td className="qb-grid-cell num">{r.kind === 'INVOICE' ? formatCents(r.balanceCents) : ''}</td>
                    <td className="qb-grid-cell num">{formatCents(r.totalCents)}</td>
                    <td className="qb-grid-cell">
                      {r.kind === 'INVOICE' || r.isVoided ? (
                        <StatusBadge status={r.status} overdue={isOverdue} voided={r.isVoided} />
                      ) : (
                        <span className="text-qb-ink-muted">{r.kind === 'PAYMENT' ? 'Closed' : 'Paid'}</span>
                      )}
                    </td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        items={[
                          ...(r.kind === 'INVOICE' ? [{ label: 'Print / Save PDF', onSelect: () => printInvoice(r.id) }] : []),
                          voidItem({ journalEntryId: r.journalEntryId, voided: r.isVoided, description: what, onVoid: voidRow }),
                        ]}
                      />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
