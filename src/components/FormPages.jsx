import { useCallback, useEffect, useRef, useState } from 'react';
import { api, errorText, printDocument, useLedgerQuery } from '../lib/api.js';
import { formatDate } from '../lib/dates.js';
import { PrintPortal } from './PrintDocuments.jsx';

// The standard PDF fonts, as the screen and the print dialog show them.
const FONT_STACKS = {
  HELVETICA: 'Helvetica, Arial, sans-serif',
  TIMES: '"Times New Roman", Times, serif',
  COURIER: '"Courier New", Courier, monospace',
};
const ANCHOR = { left: 'start', right: 'end', center: 'middle' };

/**
 * One page of a laid-out form (see forms.rs), drawn as SVG at the page's size in points. The same
 * operations are written to the PDF, so the preview and the printout match the file.
 */
export function FormSheet({ page, logo, className = '' }) {
  return (
    <svg
      className={`form-sheet ${className}`}
      viewBox={`0 0 ${page.width} ${page.height}`}
      xmlns="http://www.w3.org/2000/svg"
      role="img"
      aria-label="Form page"
    >
      <rect x="0" y="0" width={page.width} height={page.height} fill="#ffffff" />
      {page.ops.map((op, i) => {
        switch (op.op) {
          case 'text':
            return (
              <text
                key={i}
                x={op.x}
                y={op.y}
                fill={op.color}
                fontFamily={FONT_STACKS[op.family]}
                fontSize={op.size}
                fontWeight={op.bold ? 700 : 400}
                textAnchor={ANCHOR[op.align]}
                xmlSpace="preserve"
              >
                {op.text}
              </text>
            );
          case 'rect':
            return <rect key={i} x={op.x} y={op.y} width={op.w} height={op.h} fill={op.fill ?? 'none'} stroke={op.stroke ?? 'none'} strokeWidth={op.lineWidth} />;
          case 'line':
            return <line key={i} x1={op.x1} y1={op.y1} x2={op.x2} y2={op.y2} stroke={op.color} strokeWidth={op.width} />;
          case 'image':
            return logo ? <image key={i} href={logo} x={op.x} y={op.y} width={op.w} height={op.h} preserveAspectRatio="xMidYMid meet" /> : null;
          default:
            return null;
        }
      })}
    </svg>
  );
}

const FORM_NAMES = { INVOICE: 'invoice', ESTIMATE: 'estimate', SALES_RECEIPT: 'sales receipt', STATEMENT: 'statement' };

/** "Emailed 10/21/2026 to ap@acme.com" (sent_at is UTC; shown as the local date). */
export function sentText(record) {
  const local = new Date(record.sentAt ?? record.lastSentAt);
  const day = Number.isNaN(local.getTime()) ? '' : formatDate(`${local.getFullYear()}-${String(local.getMonth() + 1).padStart(2, '0')}-${String(local.getDate()).padStart(2, '0')}`);
  const how = record.method === 'MAIL_APP' ? ' (opened in mail app)' : '';
  return `Emailed ${day} to ${record.recipients ?? record.lastRecipients}${how}`;
}

/** Each form's email status (last send, Email later) by document id, kept live. */
export function useEmailStatuses(kind) {
  const { data } = useLedgerQuery(useCallback(() => api.listEmailStatuses(kind), [kind]));
  return new Map((data ?? []).map((s) => [s.documentId, s]));
}

/** The small marks beside a form's number: ✉ when emailed, a chip while it waits in Email later. */
export function EmailMarks({ status }) {
  if (!status) return null;
  return (
    <>
      {status.lastSentAt && (
        <span className="ml-1 text-qb-ink-muted" title={sentText(status) + (status.sendCount > 1 ? ` · sent ${status.sendCount} times` : '')} aria-label={sentText(status)}>
          ✉
        </span>
      )}
      {status.queued && (
        <span className="ml-1 rounded-xs bg-qb-green-tint px-1 text-2xs font-semibold text-qb-green-active" title="Marked Email later: send it from Sales › Send Forms">
          Email later
        </span>
      )}
    </>
  );
}

/** What the email dialog reports once the message is open in the mail app. */
function composedText(outcome, fileName) {
  return outcome.kind === 'attached'
    ? `A new message is open in ${outcome.app} with ${fileName} attached. Review it there and send.`
    : `Your mail app opened a new message. Attach ${fileName}, shown in your file manager, then send.`;
}

/** Edits the email for a document, then hands it to the mail app with the PDF attached. */
export function EmailFormDialog({ kind, id, period = null, onClose, onNotice }) {
  const [draft, setDraft] = useState(null);
  const [history, setHistory] = useState([]);
  // Set when sending from LocalBooks (SMTP) is turned on: { fromEmail }.
  const [smtp, setSmtp] = useState(null);
  const [method, setMethod] = useState('MAIL_APP');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const toRef = useRef(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.getEmailDraft(kind, id, period), api.getSmtpSettings(), api.getEmailHistory(kind, id)])
      .then(([d, status, sent]) => {
        if (cancelled) return;
        setHistory(sent);
        if (status.settings.enabled) {
          setSmtp({ fromEmail: status.settings.fromEmail });
          setMethod('SMTP');
        }
        setDraft(d);
      })
      .catch((err) => !cancelled && setError(errorText(err)));
    return () => {
      cancelled = true;
    };
  }, [kind, id, period?.startDate, period?.endDate]);
  useEffect(() => {
    if (draft) toRef.current?.focus();
  }, [draft]);
  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && !busy && onClose();
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [busy, onClose]);

  const set = (key) => (e) => setDraft((d) => ({ ...d, [key]: e.target.value }));
  const send = async () => {
    setBusy(true);
    setError(null);
    try {
      const email = { to: draft.to, subject: draft.subject, body: draft.body };
      if (method === 'SMTP') {
        await api.emailFormWithSmtp(kind, id, email, period);
        onNotice({ kind: 'success', text: `Sent ${draft.fileName} to ${draft.to}.` });
      } else {
        const outcome = await api.emailFormWithMailApp(kind, id, email, period);
        onNotice({ kind: 'success', text: composedText(outcome, draft.fileName) });
      }
      onClose();
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-qb-nav/40 backdrop-blur-[2px]">
      <div role="dialog" aria-modal="true" aria-labelledby="email-form-title" className="w-[560px] rounded-xs bg-white shadow-popover">
        <div className="border-b border-qb-border px-3 py-1.5">
          <h2 id="email-form-title" className="text-md font-semibold">
            Email {FORM_NAMES[kind] ?? 'form'}
          </h2>
        </div>
        {!draft ? (
          <div className="px-3 py-2 text-sm text-qb-ink-muted">{error ? <span className="font-semibold text-qb-negative">{error}</span> : 'Loading…'}</div>
        ) : (
          <div className="flex flex-col gap-1.5 px-3 py-2 text-sm">
            <label className="grid grid-cols-[64px_minmax(0,1fr)] items-center gap-2">
              <span className="text-xs font-semibold text-qb-ink-muted">To</span>
              <input ref={toRef} className="qb-input w-full" value={draft.to} onChange={set('to')} placeholder="customer@example.com, another@example.com" />
            </label>
            <label className="grid grid-cols-[64px_minmax(0,1fr)] items-center gap-2">
              <span className="text-xs font-semibold text-qb-ink-muted">Subject</span>
              <input className="qb-input w-full" value={draft.subject} onChange={set('subject')} maxLength={200} />
            </label>
            <textarea aria-label="Message" rows={9} className="qb-input h-auto w-full py-0.5" value={draft.body} onChange={set('body')} />
            <div className="flex items-center gap-1.5 text-xs text-qb-ink-muted">
              <span className="rounded-xs border border-qb-border bg-qb-canvas px-1.5 py-0.5 font-semibold text-qb-ink">📎 {draft.fileName}</span>
              <span>{method === 'SMTP' ? `sent from ${smtp.fromEmail}` : 'opens in your mail app as a new message for you to review and send'}</span>
            </div>
            {smtp && (
              <div className="flex gap-3 text-xs" role="radiogroup" aria-label="How to send">
                <label className="flex items-center gap-1">
                  <input type="radio" name="email-send-method" checked={method === 'SMTP'} onChange={() => setMethod('SMTP')} />
                  Send from LocalBooks
                </label>
                <label className="flex items-center gap-1">
                  <input type="radio" name="email-send-method" checked={method === 'MAIL_APP'} onChange={() => setMethod('MAIL_APP')} />
                  Open in my mail app instead
                </label>
              </div>
            )}
            {history.length > 0 && (
              <div className="rounded-xs border border-qb-border bg-qb-canvas px-2 py-1 text-xs text-qb-ink-muted">
                {history.slice(0, 3).map((h, i) => (
                  <div key={i}>{sentText(h)}</div>
                ))}
                {history.length > 3 && <div>…and {history.length - 3} earlier</div>}
              </div>
            )}
            {error && <p className="font-semibold text-qb-negative">{error}</p>}
          </div>
        )}
        <div className="flex justify-end gap-2 border-t border-qb-border px-3 py-1.5">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={send} disabled={busy || !draft}>
            {method === 'SMTP' ? (busy ? 'Sending…' : 'Send') : busy ? 'Opening…' : 'Open in mail app'}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Prints, saves and emails forms with their customer's form style. Returns [elements to render,
 * { print(kind, id, period), printMany([{ kind, id, period }]), savePdf(kind, id, period),
 * email(kind, id) }]; `period` is for statements. Results are reported through `onNotice`.
 */
export function useFormPrinter(onNotice) {
  // The forms to print, each { pages, logo }.
  const [forms, setForms] = useState(null);

  useEffect(() => {
    if (!forms) return undefined;
    // Once printed, empty the print root so printing the screen itself works again.
    const clear = () => setForms(null);
    window.addEventListener('afterprint', clear);
    // Let React commit the pages (and the logo image decode) before the dialog snapshots the page.
    const timer = setTimeout(printDocument, 80);
    return () => {
      clearTimeout(timer);
      window.removeEventListener('afterprint', clear);
    };
  }, [forms]);

  const printMany = useCallback(
    async (list) => {
      try {
        setForms(await Promise.all(list.map((f) => api.renderForm(f.kind, f.id, f.period ?? null))));
      } catch (err) {
        onNotice({ kind: 'error', text: errorText(err) });
      }
    },
    [onNotice],
  );
  const print = useCallback((kind, id, period = null) => printMany([{ kind, id, period }]), [printMany]);

  const savePdf = useCallback(
    async (kind, id, period = null) => {
      try {
        const path = await api.saveFormPdf(kind, id, period);
        if (path) onNotice({ kind: 'success', text: `Saved ${path}` });
      } catch (err) {
        onNotice({ kind: 'error', text: errorText(err) });
      }
    },
    [onNotice],
  );

  const [emailing, setEmailing] = useState(null);
  const email = useCallback((kind, id, period = null) => setEmailing({ kind, id, period }), []);
  const closeEmail = useCallback(() => setEmailing(null), []);

  const portal = (
    <>
      {forms && (
        <PrintPortal>
          {forms.flatMap((form, f) => form.pages.map((page, i) => <FormSheet key={`${f}-${i}`} page={page} logo={form.logo} />))}
        </PrintPortal>
      )}
      {emailing && <EmailFormDialog kind={emailing.kind} id={emailing.id} period={emailing.period} onClose={closeEmail} onNotice={onNotice} />}
    </>
  );
  const setLater = useCallback(
    async (kind, id, queued) => {
      try {
        await api.setEmailLater(kind, id, queued);
        onNotice({ kind: 'success', text: queued ? 'Marked Email later. Send it from Sales › Send Forms.' : 'Removed from Email later.' });
      } catch (err) {
        onNotice({ kind: 'error', text: errorText(err) });
      }
    },
    [onNotice],
  );
  return [portal, { print, printMany, savePdf, email, setLater }];
}

/** Row-menu entries for a document's form; `status` is its entry from useEmailStatuses. */
export function formMenuItems(kind, id, actions, status = null) {
  return [
    { label: 'Print…', onSelect: () => actions.print(kind, id) },
    { label: 'Save as PDF…', onSelect: () => actions.savePdf(kind, id) },
    { label: 'Email…', onSelect: () => actions.email(kind, id) },
    // Statements cover a period chosen when they are sent, so they are not queued.
    kind === 'STATEMENT'
      ? null
      : status?.queued
      ? { label: 'Remove from Email later', onSelect: () => actions.setLater(kind, id, false) }
      : { label: 'Email later', hint: 'Queue it in Sales › Send Forms', onSelect: () => actions.setLater(kind, id, true) },
  ].filter(Boolean);
}
