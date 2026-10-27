import { useEffect, useMemo, useState } from 'react';
import { accountLabel } from './AccountSelect.jsx';
import SplitGrid, { blankSplit, summarizeSplits } from './SplitGrid.jsx';
import Notice from './Notice.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { RefundDialog } from './CreditMemos.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';

const isCategory = (a) =>
  !a.isSystem && (a.classification === 'EXPENSE' || ['FIXED_ASSET', 'OTHER_ASSET', 'OTHER_CURRENT_ASSET'].includes(a.accountType));

function VendorCreditForm({ accounts, onPosted, onCancel }) {
  const vendors = useLedgerQuery(api.listVendors);
  const defaultCategory = useMemo(() => accounts.find((a) => a.isActive && a.classification === 'EXPENSE')?.id ?? null, [accounts]);
  const [vendor, setVendor] = useState('');
  const [reference, setReference] = useState('');
  const [date, setDate] = useState(todayIso());
  const [memo, setMemo] = useState('');
  const [rows, setRows] = useState(() => [blankSplit(null)]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const formClass = useFormClass(accounts);

  useEffect(() => {
    if (defaultCategory !== null) setRows((rs) => rs.map((r) => (r.accountId === null ? { ...r, accountId: defaultCategory } : r)));
  }, [defaultCategory]);

  const { lines, invalid, total } = summarizeSplits(rows);

  const save = async () => {
    if (!vendor.trim()) return setError('Enter a vendor.');
    if (invalid > 0) return setError(`${invalid} line(s) need a category and an amount above zero.`);
    if (lines.length === 0) return setError('Add at least one line.');
    const postedLines = formClass.apply(lines.map((l) => ({ accountId: l.accountId, amountCents: l.amountCents, memo: l.memo, classId: l.classId ?? null, customerId: l.customerId ?? null })));
    const warning = formClass.missing(postedLines);
    if (warning) return setError(warning);
    setBusy(true);
    try {
      const credit = await api.createVendorCredit({ vendorName: vendor.trim(), reference: reference.trim() || null, creditDate: date, memo: memo.trim() || null, lines: postedLines });
      onPosted(`Vendor credit from ${credit.vendorName} posted: Dr Accounts Payable ${formatCents(credit.totalCents)} / Cr ${lines.length} line${lines.length === 1 ? '' : 's'}.`);
      setVendor('');
      setReference('');
      setMemo('');
      setRows([blankSplit(defaultCategory)]);
      formClass.reset();
      setError(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">Vendor credit</h2>
        <label className="w-[220px]">
          <span className="qb-label">Vendor</span>
          <input autoFocus className="qb-input w-full" value={vendor} onChange={(e) => setVendor(e.target.value)} list="credit-vendors" placeholder="Vendor name" />
          <datalist id="credit-vendors">
            {(vendors.data ?? []).filter((v) => v.isActive).map((v) => (
              <option key={v.id} value={v.name} />
            ))}
          </datalist>
        </label>
        <label>
          <span className="qb-label">Ref no.</span>
          <input className="qb-input w-[100px]" value={reference} onChange={(e) => setReference(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Date</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Memo</span>
          <input className="qb-input w-[220px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Returned goods, overcharge…" />
        </label>
        {formClass.header}
        <div className="ml-auto text-right">
          <div className="qb-label">Credit</div>
          <div className="num text-3xl font-semibold leading-none">{formatCents(total)}</div>
        </div>
      </div>
      <SplitGrid rows={rows} setRows={setRows} accounts={accounts} filter={isCategory} defaultAccountId={defaultCategory} withClasses withJobs />
      <div className="flex items-center gap-2 px-2 py-1">
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
            {busy ? 'Posting…' : 'Save & Post'}
          </button>
        </div>
      </div>
    </section>
  );
}

function ApplyToBills({ credit, onClose, onDone }) {
  const bills = useLedgerQuery(api.listBills);
  const applied = useLedgerQuery(() => api.listVendorCreditApplications(credit.id));
  const open = (bills.data ?? []).filter((b) => b.vendorId === credit.vendorId && !b.isVoided && (b.status === 'UNPAID' || b.status === 'PARTIAL'));
  const [amounts, setAmounts] = useState({});
  const [date, setDate] = useState(todayIso());
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!bills.data) return;
    let left = credit.remainingCents;
    const next = {};
    for (const b of [...open].sort((x, y) => x.dueDate.localeCompare(y.dueDate))) {
      const take = Math.min(left, b.balanceCents);
      if (take > 0) next[b.id] = centsToInput(take);
      left -= take;
    }
    setAmounts(next);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bills.data, credit.remainingCents]);

  const parsed = Object.entries(amounts)
    .map(([id, text]) => [Number(id), text.trim() === '' ? 0 : parseMoneyToCents(text)])
    .filter(([, c]) => c !== 0);
  const total = parsed.reduce((s, [, c]) => s + (c ?? 0), 0);

  const apply = async () => {
    if (parsed.some(([, c]) => c === null || c < 0)) return setError('Enter valid amounts.');
    if (total > credit.remainingCents) return setError(`Only ${formatCents(credit.remainingCents)} of credit is left.`);
    setBusy(true);
    try {
      for (const [billId, cents] of parsed) {
        await api.applyVendorCredit({ creditId: credit.id, documentId: billId, amountCents: cents, appliedDate: date });
      }
      onDone(`Applied ${formatCents(total)} of the ${credit.vendorName} credit to bills.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  const remove = async (application) => {
    try {
      await api.removeVendorCredit(application.id);
      applied.reload();
      bills.reload();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Dialog title={`Apply vendor credit · ${credit.vendorName}`} onClose={onClose}>
      <div className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="flex items-end gap-3">
          <span className="text-sm">
            Credit left <span className="num font-semibold">{formatCents(credit.remainingCents)}</span>
          </span>
          <label className="ml-auto flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Date applied</span>
            <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
        </div>
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Bill</th>
              <th className="qb-grid-head">Due</th>
              <th className="qb-grid-head text-right">Open balance</th>
              <th className="qb-grid-head text-right">Apply</th>
            </tr>
          </thead>
          <tbody>
            {open.length === 0 && (
              <tr>
                <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">{bills.data ? `No open bills from ${credit.vendorName}.` : 'Loading…'}</td>
              </tr>
            )}
            {open.map((b) => (
              <tr key={b.id} className="qb-grid-row">
                <td className="qb-grid-cell">{b.billNumber ? `#${b.billNumber}` : formatDate(b.billDate)}</td>
                <td className="qb-grid-cell">{formatDate(b.dueDate)}</td>
                <td className="qb-grid-cell num">{formatCents(b.balanceCents)}</td>
                <td className="qb-grid-cell-edit w-[120px]">
                  <input className="num" value={amounts[b.id] ?? ''} onChange={(e) => setAmounts((a) => ({ ...a, [b.id]: e.target.value }))} inputMode="decimal" aria-label="Apply to bill" />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {(applied.data ?? []).map((a) => (
          <div key={a.id} className="flex items-center gap-2 border-b border-qb-border py-0.5">
            <span>{a.documentLabel}</span>
            <span className="text-qb-ink-muted">{formatDate(a.appliedDate)}</span>
            <span className="num ml-auto font-semibold">{formatCents(a.amountCents)}</span>
            <button type="button" className="text-qb-info hover:underline" onClick={() => remove(a)}>
              Remove
            </button>
          </div>
        ))}
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Close</button>
          <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" onClick={apply} disabled={busy || total === 0}>
            Apply {total > 0 ? formatCents(total) : ''}
          </button>
        </div>
      </div>
    </Dialog>
  );
}

export default function VendorCredits({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listVendorCredits);
  const sorter = useSort(data, {
    date: (c) => c.creditDate,
    vendor: (c) => c.vendorName,
    reference: (c) => c.reference,
    total: (c) => c.totalCents,
    left: (c) => (c.isVoided ? null : c.remainingCents),
    status: (c) => (c.isVoided ? 'Voided' : c.remainingCents === 0 ? 'Used' : c.remainingCents < c.totalCents ? 'Partly used' : 'Open'),
  }, { storageKey: 'vendor-credits' });
  const [composing, setComposing] = useState(intent?.action === 'vendor-credit');
  const [dialog, setDialog] = useState(null);
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (intent?.action === 'vendor-credit') setComposing(true);
  }, [intent]);

  const done = (text) => {
    setDialog(null);
    setNotice({ kind: 'success', text });
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {composing && <VendorCreditForm accounts={accounts} onCancel={() => setComposing(false)} onPosted={(text) => setNotice({ kind: 'success', text })} />}
      {dialog?.mode === 'apply' && <ApplyToBills credit={dialog.credit} onClose={() => setDialog(null)} onDone={done} />}
      {dialog?.mode === 'refund' && (
        <RefundDialog
          title={`Vendor refund · ${dialog.credit.vendorName}`}
          remainingCents={dialog.credit.remainingCents}
          accounts={accounts}
          direction="in"
          onClose={() => setDialog(null)}
          onRefund={async (refund) => {
            await api.refundVendorCredit({ creditId: dialog.credit.id, ...refund });
            done(`Recorded ${formatCents(refund.amountCents)} refunded by ${dialog.credit.vendorName}: Dr ${accountLabel(accounts, refund.accountId)} / Cr Accounts Payable.`);
          }}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Vendor credits</h2>
          <span className="text-xs text-qb-ink-muted">Credit a vendor gave you: apply it to their bills, or record their refund.</span>
          {!composing && (
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setComposing(true)}>
              New vendor credit
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[92px]" />
              <col />
              <col className="w-[110px]" />
              <col className="w-[120px]" />
              <col className="w-[120px]" />
              <col className="w-[110px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <SortTh sorter={sorter} col="date">Date</SortTh>
                <SortTh sorter={sorter} col="vendor">Vendor</SortTh>
                <SortTh sorter={sorter} col="reference">Ref no.</SortTh>
                <SortTh sorter={sorter} col="total" className="text-right">Total</SortTh>
                <SortTh sorter={sorter} col="left" className="text-right">Credit left</SortTh>
                <SortTh sorter={sorter} col="status">Status</SortTh>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && data.length === 0 && (
                <tr>
                  <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No vendor credits yet.</td>
                </tr>
              )}
              {sorter.sorted.map((c) => (
                <tr key={c.id} className={`qb-grid-row ${c.isVoided ? 'text-qb-ink-faint' : ''}`}>
                  <td className="qb-grid-cell">{formatDate(c.creditDate)}</td>
                  <td className="qb-grid-cell font-semibold">{c.vendorName}</td>
                  <td className="qb-grid-cell">{c.reference ?? ''}</td>
                  <td className="qb-grid-cell num">{formatCents(c.totalCents)}</td>
                  <td className="qb-grid-cell num font-semibold">{c.isVoided ? '' : formatCents(c.remainingCents)}</td>
                  <td className="qb-grid-cell text-sm">{c.isVoided ? 'Voided' : c.remainingCents === 0 ? 'Used' : c.remainingCents < c.totalCents ? 'Partly used' : 'Open'}</td>
                  <td className="qb-grid-cell cell-tight text-center">
                    <RowMenu
                      label={`Actions for the ${c.vendorName} credit`}
                      items={[
                        { label: 'Apply to bills…', disabled: c.isVoided || c.remainingCents === 0, onSelect: () => setDialog({ mode: 'apply', credit: c }) },
                        { label: 'Record vendor refund…', disabled: c.isVoided || c.remainingCents === 0, onSelect: () => setDialog({ mode: 'refund', credit: c }) },
                        voidItem({
                          journalEntryId: c.journalEntryId,
                          voided: c.isVoided,
                          description: `Vendor credit from ${c.vendorName} (${formatCents(c.totalCents)}).`,
                          onVoid: async (id) => {
                            await api.voidTransaction(id);
                            setNotice({ kind: 'success', text: `Voided the ${c.vendorName} credit.` });
                          },
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
  );
}
