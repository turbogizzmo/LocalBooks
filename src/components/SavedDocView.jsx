import { useEffect, useState } from 'react';
import FormToolbar, { fileButtons, navButtons } from './FormToolbar.jsx';
import Attachments from './Attachments.jsx';
import Notice from './Notice.jsx';
import { FormSheet, formMenuItems, useEmailStatuses, useFormPrinter } from './FormPages.jsx';
import { TransactionVoucher, useVoucherActions } from './VoucherPrint.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { api, errorText } from '../lib/api.js';
import { stepButtons } from '../lib/formStep.js';

/**
 * Which saved transaction a screen shows in place of its form. `ordered` is the saved ones, oldest
 * first; the one shown is kept by id, so it follows the list as it refreshes (and closes if it
 * leaves it). `step` feeds the saved view's Previous / Next; `fromNew(blocked)` the new form's,
 * where Previous opens the newest.
 */
export function useSavedDoc(ordered) {
  const [id, setId] = useState(null);
  const current = id === null ? null : (ordered ?? []).find((r) => r.id === id) ?? null;
  const open = (row) => setId(row.id);
  const close = () => setId(null);
  return {
    current,
    open,
    close,
    step: stepButtons(ordered, current?.id ?? null, open),
    fromNew: (blocked = false) => stepButtons(ordered, null, open, { blocked }),
  };
}

/** A saved customer form (sales receipt, estimate) drawn as it prints. */
export function FormPreview({ kind, id }) {
  const [form, setForm] = useState(null);
  const [error, setError] = useState(null);
  useEffect(() => {
    let live = true;
    setForm(null);
    setError(null);
    api.renderForm(kind, id, null).then(
      (f) => live && setForm(f),
      (err) => live && setError(errorText(err)),
    );
    return () => {
      live = false;
    };
  }, [kind, id]);
  if (error) return <Notice kind="error" text={error} />;
  if (!form) return <div className="px-2 py-1 text-sm text-qb-ink-muted">Loading…</div>;
  return (
    <div className="mx-auto flex max-w-[820px] flex-col gap-3">
      {form.pages.map((page, i) => (
        <FormSheet key={i} page={page} logo={form.logo} className="w-full bg-white shadow-md" />
      ))}
    </div>
  );
}

/** A saved posted transaction drawn as its voucher: header and the accounts it posted to. */
export function VoucherPreview({ doc }) {
  const company = useCompany();
  const [lines, setLines] = useState(null);
  const [error, setError] = useState(null);
  useEffect(() => {
    let live = true;
    setLines(null);
    setError(null);
    if (!doc.entryId) return undefined;
    api.getJournalEntryLines(doc.entryId).then(
      (l) => live && setLines(l),
      (err) => live && setError(errorText(err)),
    );
    return () => {
      live = false;
    };
  }, [doc.entryId]);
  if (error) return <Notice kind="error" text={error} />;
  if (!lines) return <div className="px-2 py-1 text-sm text-qb-ink-muted">Loading…</div>;
  return (
    <div className="mx-auto max-w-[820px] bg-white p-6 shadow-md">
      <TransactionVoucher company={company} doc={doc} lines={lines} />
    </div>
  );
}

/**
 * A saved transaction shown where its form goes, with the form's toolbar: Previous / Next step
 * through the others, New goes back to a blank form, and Void, Print, Email and Attach act on this
 * one. Forms that can't load a saved transaction for editing use it (sales receipts, estimates,
 * credit memos, vendor credits, purchase orders, deposits and transfers).
 *
 *   label      what it is ("Sales receipt"), for the toolbar and heading
 *   title      the heading ("Sales receipt #12 · Acme")
 *   step       stepButtons(...) for Previous / Next
 *   form       { kind, id }: a customer form, previewed, printed and emailed as designed
 *   voucher    the voucher doc (TransactionVoucher): previewed and printed as a voucher
 *   onPrint    prints it some other way (purchase orders), with `children` as its preview
 *   entryId    its journal entry, for Attach File (none for estimates and purchase orders)
 *   void       { onVoid, disabled, hint, confirm }, when it can be voided here
 *   notes      lines shown above it (status, what is left to apply)
 *   extra      more toolbar groups
 */
export default function SavedDocView({ label, title, step, onFind, onNew, form, voucher, onPrint, entryId, void: voiding, notes = [], extra = [], children, onNotice }) {
  const [notice, setNotice] = useState(null);
  const say = onNotice ?? setNotice;
  const [printPortal, forms] = useFormPrinter(say);
  const [voucherElements, voucherItems, printVoucher] = useVoucherActions(say);
  // Hooks run either way; a voucher's lookup is unused (any form kind is a valid query).
  const emailed = useEmailStatuses(form?.kind ?? 'INVOICE');
  const [showAttachments, setShowAttachments] = useState(false);
  const [busy, setBusy] = useState(false);
  // Another transaction opened: its attachments start closed.
  const key = form ? `${form.kind}-${form.id}` : voucher ? `v-${voucher.entryId}` : title;
  useEffect(() => setShowAttachments(false), [key]);

  // A voucher (a posted transaction) saves as a PDF and emails to the name on it too.
  const [voucherPrint, voucherEmail] = voucher && !onPrint ? voucherItems(voucher, { label }) : [null, null];
  const printItem = form
    ? { id: 'print', label: 'Print', icon: 'Print', menu: [{ label: 'Print…', onSelect: () => forms.print(form.kind, form.id) }, { label: 'Save as PDF…', onSelect: () => forms.savePdf(form.kind, form.id) }] }
    : (voucherPrint ?? { id: 'print', label: 'Print', icon: 'Print', hint: `Print this ${label.toLowerCase()}`, onClick: () => (onPrint ? onPrint() : printVoucher(voucher)) });
  const emailItem = !form ? voucherEmail : {
    id: 'email',
    label: 'Email',
    icon: 'Email',
    menu: formMenuItems(form.kind, form.id, forms, emailed.get(form.id)).filter((m) => m.label.startsWith('Email') || m.label.startsWith('Remove')),
  };
  const groups = [
    navButtons({ onFind, ...step }),
    fileButtons({
      onNew,
      busy,
      onVoid: voiding
        ? async () => {
            setBusy(true);
            try {
              await voiding.onVoid();
            } catch (err) {
              say({ kind: 'error', text: errorText(err) });
            } finally {
              setBusy(false);
            }
          }
        : undefined,
      voidDisabled: voiding?.disabled,
      voidHint: voiding?.hint,
      voidConfirm: voiding?.confirm,
    }),
    [
      printItem,
      emailItem,
      entryId
        ? { id: 'attach', label: 'Attach File', icon: 'Attach', hint: 'Show and add files attached to it', onClick: () => setShowAttachments((v) => !v) }
        : null,
    ].filter(Boolean),
    ...extra,
  ];

  return (
    <section className="qb-panel flex min-h-0 flex-none flex-col" aria-label={title}>
      {printPortal}
      {voucherElements}
      <FormToolbar label={label} groups={groups} />
      <div className="flex items-center gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="text-md font-semibold">{title}</h2>
        <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={onNew}>
          New {label.toLowerCase()}
        </button>
      </div>
      {notes.filter(Boolean).map((n, i) => (
        <Notice key={i} kind={n.kind ?? 'info'} text={n.text} />
      ))}
      {!onNotice && notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {showAttachments && entryId && (
        <div className="border-b border-qb-border bg-qb-canvas">
          <Attachments entryId={entryId} />
        </div>
      )}
      <div className="max-h-[56vh] overflow-auto bg-qb-canvas p-3 scrollbar-thin">
        {children ?? (form ? <FormPreview kind={form.kind} id={form.id} /> : <VoucherPreview doc={voucher} />)}
      </div>
    </section>
  );
}
