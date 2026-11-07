// File › Print Forms › Checks… and Invoices…: print the checks and invoices marked Print later.
//
// Checks take numbers in order from the first number on the check stock. Nothing is saved until
// the user says which ones came out right: those keep their numbers and leave the queue; the rest
// stay queued without a number, to print again.
import { useEffect, useMemo, useRef, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { CheckDocument, PrintPortal, useCompany } from './PrintDocuments.jsx';
import { useFormPrinter } from './FormPages.jsx';
import { api, errorText, printDocument, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatMoney } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

/** "1007" + 2 → "1009"; keeps leading zeros ("0098" + 3 → "0101"); null when it isn't a number. */
export function numberAfter(first, offset) {
  const text = first.trim();
  if (!/^\d{1,18}$/.test(text)) return null;
  const next = String(BigInt(text) + BigInt(offset));
  return next.length < text.length ? next.padStart(text.length, '0') : next;
}

/** Asks which documents printed correctly; `onDone(okIds)` with the ids that did. */
function DidTheyPrint({ items, describe, onDone, onCancel, busy, error }) {
  const [partly, setPartly] = useState(false);
  const [ok, setOk] = useState(() => new Set(items.map((i) => i.id)));
  const yesRef = useRef(null);
  useEffect(() => yesRef.current?.focus(), []);
  const toggle = (id) =>
    setOk((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-qb-nav/40 backdrop-blur-[2px]">
      <div role="alertdialog" aria-modal="true" aria-labelledby="did-print-title" className="w-[460px] rounded-xs bg-white shadow-popover">
        <div className="border-b border-qb-border px-3 py-1.5">
          <h2 id="did-print-title" className="text-md font-semibold">Did they all print?</h2>
        </div>
        <div className="space-y-2 px-3 py-2 text-sm">
          {!partly ? (
            <p>
              If every {items.length === 1 ? 'form' : `one of the ${items.length} forms`} came out right, choose <b>Yes, all printed</b>. If a sheet jammed or printed
              wrong, choose <b>Some didn’t</b> and untick those; they stay in the queue to print again.
            </p>
          ) : (
            <>
              <p>Tick the ones that printed correctly.</p>
              <ul className="max-h-[260px] overflow-auto rounded-xs border border-qb-border">
                {items.map((item) => (
                  <li key={item.id} className="border-b border-qb-border px-2 py-0.5 last:border-b-0">
                    <label className="flex items-center gap-2">
                      <input type="checkbox" className="accent-qb-green" checked={ok.has(item.id)} onChange={() => toggle(item.id)} />
                      {describe(item)}
                    </label>
                  </li>
                ))}
              </ul>
            </>
          )}
          {error && <p className="font-semibold text-qb-negative">{error}</p>}
        </div>
        <div className="flex justify-end gap-2 border-t border-qb-border px-3 py-1.5">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy} title="Keep them all in the queue">
            Cancel
          </button>
          {!partly ? (
            <>
              <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setPartly(true)} disabled={busy}>
                Some didn’t
              </button>
              <button ref={yesRef} type="button" className="qb-btn qb-btn-primary" onClick={() => onDone(items.map((i) => i.id))} disabled={busy}>
                {busy ? 'Saving…' : 'Yes, all printed'}
              </button>
            </>
          ) : (
            <button type="button" className="qb-btn qb-btn-primary" onClick={() => onDone(items.filter((i) => ok.has(i.id)).map((i) => i.id))} disabled={busy}>
              {busy ? 'Saving…' : `Save (${ok.size} printed)`}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function useQueuePicker(rows) {
  const [picked, setPicked] = useState(() => new Set());
  // New queue contents start ticked, as the desktop dialog selects everything.
  const key = rows.map((r) => r.id).join(',');
  useEffect(() => setPicked(new Set(rows.map((r) => r.id))), [key]); // eslint-disable-line react-hooks/exhaustive-deps
  const toggle = (id) =>
    setPicked((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const all = rows.length > 0 && rows.every((r) => picked.has(r.id));
  const toggleAll = () => setPicked(all ? new Set() : new Set(rows.map((r) => r.id)));
  return { picked, toggle, all, toggleAll, chosen: rows.filter((r) => picked.has(r.id)) };
}

export function PrintChecks({ accounts }) {
  const company = useCompany();
  const { data, error } = useLedgerQuery(api.listChecksToPrint);
  const queued = useMemo(() => data ?? [], [data]);
  const banks = useMemo(() => [...new Set(queued.map((c) => c.paymentAccountId))], [queued]);
  const [bankId, setBankId] = useState(null);
  useEffect(() => {
    if (bankId === null || !banks.includes(bankId)) setBankId(banks[0] ?? null);
  }, [banks, bankId]);
  const rows = useMemo(() => queued.filter((c) => c.paymentAccountId === bankId), [queued, bankId]);
  const picker = useQueuePicker(rows);
  const [first, setFirst] = useState('');
  useEffect(() => {
    if (bankId === null) return undefined;
    let cancelled = false;
    api.nextCheckNumber(bankId).then((n) => !cancelled && setFirst(n), () => {});
    return () => {
      cancelled = true;
    };
  }, [bankId, data]);
  const [printing, setPrinting] = useState(null); // the checks with their planned numbers
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  const [askError, setAskError] = useState(null);

  const numbered = picker.chosen.map((c, i) => ({ ...c, checkNumber: numberAfter(first, i) }));
  const print = async () => {
    if (numbered.length === 0) return setNotice({ kind: 'error', text: 'Tick the checks to print.' });
    if (numbered.some((c) => c.checkNumber === null)) return setNotice({ kind: 'error', text: 'Enter the number printed on the first check in the printer, such as 1001.' });
    setNotice(null);
    setPrinting(numbered);
    // Let React render the checks into the print root before the dialog opens.
    await new Promise((resolve) => setTimeout(resolve, 80));
    await printDocument(numbered.length === 1 ? `Check ${numbered[0].checkNumber}` : 'Checks');
    setAskError(null);
    setAsking(true);
  };
  const finish = async (okIds) => {
    setBusy(true);
    try {
      const printed = printing.filter((c) => okIds.includes(c.id)).map((c) => ({ expenseId: c.id, checkNumber: c.checkNumber }));
      await api.markChecksPrinted(printed);
      const left = printing.length - printed.length;
      setNotice({
        kind: 'success',
        text:
          printed.length === 0
            ? 'Nothing was saved: the checks stay in the queue.'
            : `${printed.length} check${printed.length === 1 ? '' : 's'} numbered ${printed[0].checkNumber}${printed.length > 1 ? `–${printed[printed.length - 1].checkNumber}` : ''} and taken out of the queue.` +
              (left ? ` ${left} stay${left === 1 ? 's' : ''} queued to print again.` : ''),
      });
      setAsking(false);
      setPrinting(null);
    } catch (err) {
      setAskError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printing && (
        <PrintPortal>
          {printing.map((check) => (
            <CheckDocument key={check.id} company={company} check={check} bankName={accountLabel(accounts, check.paymentAccountId)} />
          ))}
        </PrintPortal>
      )}
      {asking && printing && (
        <DidTheyPrint
          items={printing}
          describe={(c) => `#${c.checkNumber} · ${c.payeeName ?? 'No payee'} · ${formatMoney(c.totalCents, c.currencyCode)}`}
          busy={busy}
          error={askError}
          onDone={finish}
          onCancel={() => {
            setAsking(false);
            setPrinting(null);
            setNotice({ kind: 'info', text: 'Nothing was saved: the checks stay in the queue.' });
          }}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load the checks to print: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
          <h2 className="mr-2 self-center text-md font-semibold">Select checks to print</h2>
          <label>
            <span className="qb-label">Bank account</span>
            <AccountSelect accounts={accounts} filter={(a) => banks.includes(a.id)} value={bankId} onChange={setBankId} placeholder="No checks waiting" className="qb-input w-[210px]" />
          </label>
          <label>
            <span className="qb-label">First check number</span>
            <input className="qb-input num w-[100px]" value={first} onChange={(e) => setFirst(e.target.value)} aria-invalid={first !== '' && numberAfter(first, 0) === null} />
          </label>
          <span className="self-center text-xs text-qb-ink-muted">
            {picker.chosen.length} of {rows.length} selected · {formatCents(picker.chosen.reduce((s, c) => s + c.homeTotalCents, 0))}
          </span>
          <button type="button" className="qb-btn qb-btn-primary ml-auto" onClick={print} disabled={picker.chosen.length === 0 || !!printing}>
            Print…
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[34px]" />
              <col className="w-[92px]" />
              <col className="w-[80px]" />
              <col />
              <col className="w-[260px]" />
              <col className="w-[112px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head text-center">
                  <input type="checkbox" className="accent-qb-green" checked={picker.all} onChange={picker.toggleAll} aria-label="Select all" />
                </th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">No.</th>
                <th className="qb-grid-head">Payee</th>
                <th className="qb-grid-head">Memo</th>
                <th className="qb-grid-head text-right">Amount</th>
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">
                    No checks are waiting to print. Tick Print later on a check (Banking › Write Checks) to queue it here.
                  </td>
                </tr>
              )}
              {rows.map((c) => {
                const index = numbered.findIndex((n) => n.id === c.id);
                return (
                  <tr key={c.id} className="qb-grid-row cursor-pointer" onClick={() => picker.toggle(c.id)}>
                    <td className="qb-grid-cell text-center">
                      <input type="checkbox" className="accent-qb-green" checked={picker.picked.has(c.id)} onChange={() => picker.toggle(c.id)} onClick={(e) => e.stopPropagation()} aria-label={`Print check to ${c.payeeName ?? 'no payee'}`} />
                    </td>
                    <td className="qb-grid-cell">{formatDate(c.expenseDate)}</td>
                    <td className="qb-grid-cell num text-qb-ink-muted">{index >= 0 ? numbered[index].checkNumber ?? '' : ''}</td>
                    <td className="qb-grid-cell font-semibold">{c.payeeName ?? <span className="font-normal text-qb-ink-faint">—</span>}</td>
                    <td className="qb-grid-cell text-qb-ink-muted">{c.memo ?? ''}</td>
                    <td className="qb-grid-cell num">{formatMoney(c.totalCents, c.currencyCode)}</td>
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

export function PrintInvoices() {
  const { data, error } = useLedgerQuery(api.listInvoicesToPrint);
  const rows = useMemo(() => data ?? [], [data]);
  const picker = useQueuePicker(rows);
  const [notice, setNotice] = useState(null);
  const [printPortal, forms] = useFormPrinter(setNotice);
  const [printed, setPrinted] = useState(null); // the invoices sent to the printer
  const [busy, setBusy] = useState(false);
  const [askError, setAskError] = useState(null);

  const print = async () => {
    if (picker.chosen.length === 0) return setNotice({ kind: 'error', text: 'Tick the invoices to print.' });
    setNotice(null);
    const chosen = picker.chosen;
    await forms.printMany(chosen.map((inv) => ({ kind: 'INVOICE', id: inv.id })));
    // The print dialog opens once the pages render; ask afterwards.
    await new Promise((resolve) => setTimeout(resolve, 400));
    setAskError(null);
    setPrinted(chosen);
  };
  const finish = async (okIds) => {
    setBusy(true);
    try {
      if (okIds.length) await api.markInvoicesPrinted(okIds);
      const left = printed.length - okIds.length;
      setNotice({
        kind: 'success',
        text:
          okIds.length === 0
            ? 'Nothing was saved: the invoices stay in the queue.'
            : `${okIds.length} invoice${okIds.length === 1 ? '' : 's'} taken out of the queue.${left ? ` ${left} stay${left === 1 ? 's' : ''} queued.` : ''}`,
      });
      setPrinted(null);
    } catch (err) {
      setAskError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printPortal}
      {printed && (
        <DidTheyPrint
          items={printed}
          describe={(inv) => `Invoice #${inv.invoiceNumber} · ${inv.customerName} · ${formatMoney(inv.totalCents, inv.currencyCode)}`}
          busy={busy}
          error={askError}
          onDone={finish}
          onCancel={() => {
            setPrinted(null);
            setNotice({ kind: 'info', text: 'Nothing was saved: the invoices stay in the queue.' });
          }}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load the invoices to print: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Select invoices to print</h2>
          <span className="text-xs text-qb-ink-muted">
            {picker.chosen.length} of {rows.length} selected · printed with each invoice’s template (Settings › Forms)
          </span>
          <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={print} disabled={picker.chosen.length === 0 || !!printed}>
            Print…
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[34px]" />
              <col className="w-[92px]" />
              <col className="w-[80px]" />
              <col />
              <col className="w-[112px]" />
              <col className="w-[112px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head text-center">
                  <input type="checkbox" className="accent-qb-green" checked={picker.all} onChange={picker.toggleAll} aria-label="Select all" />
                </th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">No.</th>
                <th className="qb-grid-head">Customer</th>
                <th className="qb-grid-head text-right">Amount</th>
                <th className="qb-grid-head text-right">Open balance</th>
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">
                    No invoices are waiting to print. Tick Print later on an invoice (Customers › Create Invoices) to queue it here.
                  </td>
                </tr>
              )}
              {rows.map((inv) => (
                <tr key={inv.id} className="qb-grid-row cursor-pointer" onClick={() => picker.toggle(inv.id)}>
                  <td className="qb-grid-cell text-center">
                    <input type="checkbox" className="accent-qb-green" checked={picker.picked.has(inv.id)} onChange={() => picker.toggle(inv.id)} onClick={(e) => e.stopPropagation()} aria-label={`Print invoice ${inv.invoiceNumber}`} />
                  </td>
                  <td className="qb-grid-cell">{formatDate(inv.issueDate)}</td>
                  <td className="qb-grid-cell">{inv.invoiceNumber}</td>
                  <td className="qb-grid-cell font-semibold">{inv.customerName}</td>
                  <td className="qb-grid-cell num">{formatMoney(inv.totalCents, inv.currencyCode)}</td>
                  <td className="qb-grid-cell num">{inv.balanceCents ? formatMoney(inv.balanceCents, inv.currencyCode) : ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
