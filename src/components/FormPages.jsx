import { useCallback, useEffect, useRef, useState } from 'react';
import { api, errorText, printDocument } from '../lib/api.js';
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

/** What the email dialog reports once the message is open in the mail app. */
function composedText(outcome, fileName) {
  return outcome.kind === 'attached'
    ? `A new message is open in ${outcome.app} with ${fileName} attached. Review it there and send.`
    : `Your mail app opened a new message. Attach ${fileName}, shown in your file manager, then send.`;
}

/** Edits the email for a document, then hands it to the mail app with the PDF attached. */
export function EmailFormDialog({ kind, id, onClose, onNotice }) {
  const [draft, setDraft] = useState(null);
  // Set when sending from LocalBooks (SMTP) is turned on: { fromEmail }.
  const [smtp, setSmtp] = useState(null);
  const [method, setMethod] = useState('MAIL_APP');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const toRef = useRef(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.getEmailDraft(kind, id), api.getSmtpSettings()])
      .then(([d, status]) => {
        if (cancelled) return;
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
  }, [kind, id]);
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
        await api.emailFormWithSmtp(kind, id, email);
        onNotice({ kind: 'success', text: `Sent ${draft.fileName} to ${draft.to}.` });
      } else {
        const outcome = await api.emailFormWithMailApp(kind, id, email);
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
            Email {kind === 'INVOICE' ? 'invoice' : 'estimate'}
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
  const email = useCallback((kind, id) => setEmailing({ kind, id }), []);
  const closeEmail = useCallback(() => setEmailing(null), []);

  const portal = (
    <>
      {forms && (
        <PrintPortal>
          {forms.flatMap((form, f) => form.pages.map((page, i) => <FormSheet key={`${f}-${i}`} page={page} logo={form.logo} />))}
        </PrintPortal>
      )}
      {emailing && <EmailFormDialog kind={emailing.kind} id={emailing.id} onClose={closeEmail} onNotice={onNotice} />}
    </>
  );
  return [portal, { print, printMany, savePdf, email }];
}

/** Row-menu entries for a document's form; `withEmail` false leaves out Email…. */
export function formMenuItems(kind, id, actions, withEmail = true) {
  return [
    { label: 'Print…', onSelect: () => actions.print(kind, id) },
    { label: 'Save as PDF…', onSelect: () => actions.savePdf(kind, id) },
    ...(withEmail ? [{ label: 'Email…', onSelect: () => actions.email(kind, id) }] : []),
  ];
}
