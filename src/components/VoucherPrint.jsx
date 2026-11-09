import { useCallback, useEffect, useState } from 'react';
import { DocHeader, PrintPortal, longDate, useCompany } from './PrintDocuments.jsx';
import { api, errorText, printDocument } from '../lib/api.js';
import { EmailFormDialog } from './FormPages.jsx';
import { formatMoney } from '../lib/money.js';
import { savedCopyText } from '../lib/plainCopy.js';

/**
 * A posted transaction printed as a voucher: its header (number, date, name, memo) and the ledger
 * lines it posted, debits and credits. Used for the forms with no customer-facing layout of their
 * own: bills, expenses and journal entries.
 */
export function TransactionVoucher({ company, doc, lines }) {
  const money = (cents) => (cents ? formatMoney(cents, null) : '');
  const debits = lines.reduce((s, l) => s + l.debitCents, 0);
  const credits = lines.reduce((s, l) => s + l.creditCents, 0);
  const meta = [
    doc.number ? ['No.', doc.number] : null,
    ['Date', longDate(doc.date)],
    doc.dueDate ? ['Due', longDate(doc.dueDate)] : null,
    doc.amountCents !== undefined && doc.amountCents !== null ? ['Amount', formatMoney(doc.amountCents, doc.currencyCode ?? null)] : null,
  ].filter(Boolean);
  return (
    <article className="doc" style={{ position: 'relative' }}>
      {doc.voided && <div className="doc-void">VOID</div>}
      <DocHeader company={company} title={doc.title} />
      <div className="doc-parties">
        <div>
          {doc.name && (
            <>
              <div className="doc-label">{doc.nameLabel ?? 'Name'}</div>
              <div className="doc-bill-to">{doc.name}</div>
            </>
          )}
          {doc.memo && <div style={{ marginTop: '6pt' }}>{doc.memo}</div>}
        </div>
        <table className="doc-meta">
          <tbody>
            {meta.map(([k, v]) => (
              <tr key={k}>
                <td>{k}</td>
                <td>{v}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <table className="doc-table">
        <colgroup>
          <col style={{ width: '2.4in' }} />
          <col />
          <col style={{ width: '1.1in' }} />
          <col style={{ width: '1.1in' }} />
        </colgroup>
        <thead>
          <tr>
            <th>Account</th>
            <th>Memo</th>
            <th className="doc-num">Debit</th>
            <th className="doc-num">Credit</th>
          </tr>
        </thead>
        <tbody>
          {lines.map((l) => (
            <tr key={l.lineNumber}>
              <td>{l.accountName}{l.className ? ` · ${l.className}` : ''}</td>
              <td>{l.memo ?? ''}</td>
              <td className="doc-num">{money(l.debitCents)}</td>
              <td className="doc-num">{money(l.creditCents)}</td>
            </tr>
          ))}
          <tr style={{ fontWeight: 700 }}>
            <td colSpan={2}>Total</td>
            <td className="doc-num">{formatMoney(debits, null)}</td>
            <td className="doc-num">{formatMoney(credits, null)}</td>
          </tr>
        </tbody>
      </table>
      {doc.footer && <p className="doc-note">{doc.footer}</p>}
    </article>
  );
}

/** A voucher's header as the backend takes it for a PDF or an email (`vouchers.rs`). */
export function voucherForPdf(doc) {
  const text = (v) => (v === null || v === undefined || v === '' ? null : String(v));
  return {
    entryId: doc.entryId,
    title: doc.title,
    number: text(doc.number),
    date: doc.date,
    dueDate: text(doc.dueDate),
    name: text(doc.name),
    nameLabel: text(doc.nameLabel),
    memo: text(doc.memo),
    amountCents: doc.amountCents ?? null,
    currencyCode: doc.currencyCode ?? null,
    voided: Boolean(doc.voided),
    footer: text(doc.footer),
  };
}

/**
 * Prints a posted transaction as a voucher. `print(doc)` takes { entryId, title, number, date,
 * name, nameLabel, memo, amountCents, currencyCode, dueDate, voided } and reads the entry's lines
 * first. Returns [portal to render, print].
 */
export function useVoucherPrinter(onNotice) {
  const company = useCompany();
  const [printing, setPrinting] = useState(null);
  // Once printed, empty the print root so printing the screen itself works again.
  useEffect(() => {
    if (!printing) return undefined;
    const clear = () => setPrinting(null);
    window.addEventListener('afterprint', clear);
    return () => window.removeEventListener('afterprint', clear);
  }, [printing]);
  const print = useCallback(
    async (doc) => {
      try {
        const lines = await api.getJournalEntryLines(doc.entryId);
        setPrinting({ doc, lines });
        // Let React render the voucher into the print root before the dialog opens.
        setTimeout(() => printDocument([doc.title, doc.number].filter(Boolean).join(' ')), 80);
      } catch (err) {
        onNotice?.({ kind: 'error', text: errorText(err) });
      }
    },
    [onNotice],
  );
  const portal = printing && (
    <PrintPortal>
      <TransactionVoucher company={company} doc={printing.doc} lines={printing.lines} />
    </PrintPortal>
  );
  return [portal, print];
}

/**
 * A voucher's Print and Email toolbar items: Print ▾ (Print…, Save as PDF…) and Email, which sends
 * the PDF to the vendor or customer named on it. `items(doc, { print, hint, label })` takes the
 * voucher doc (null while the transaction isn't saved) and, optionally, another way to print it
 * (a check prints on check stock). Returns [elements to render, items].
 */
export function useVoucherActions(onNotice) {
  const [portal, printVoucher] = useVoucherPrinter(onNotice);
  const [emailing, setEmailing] = useState(null);
  const savePdf = async (doc) => {
    try {
      const path = await api.saveVoucherPdf(voucherForPdf(doc));
      if (path) onNotice?.({ kind: 'success', text: savedCopyText(path) });
    } catch (err) {
      onNotice?.({ kind: 'error', text: errorText(err) });
    }
  };
  const items = (doc, { print = null, hint = 'Save it first, then print it', label = doc?.title ?? 'it' } = {}) => {
    const saved = Boolean(doc?.entryId);
    const what = label.toLowerCase();
    return [
      {
        id: 'print',
        label: 'Print',
        icon: 'Print',
        hint: saved ? `Print this ${what}, or save it as a PDF` : hint,
        disabled: !saved,
        menu: saved ? [{ label: 'Print…', onSelect: () => (print ? print() : printVoucher(doc)) }, { label: 'Save as PDF…', onSelect: () => savePdf(doc) }] : undefined,
      },
      {
        id: 'email',
        label: 'Email',
        icon: 'Email',
        hint: saved ? `Email this ${what} as a PDF${doc.name ? ` to ${doc.name}` : ''}` : hint,
        disabled: !saved,
        onClick: () => setEmailing(voucherForPdf(doc)),
      },
    ];
  };
  const elements = (
    <>
      {portal}
      {/* `emailing` is set once per Email click, so the dialog's draft loads once. */}
      {emailing && <EmailFormDialog voucher={emailing} onClose={() => setEmailing(null)} onNotice={(n) => onNotice?.(n)} />}
    </>
  );
  return [elements, items, printVoucher];
}
