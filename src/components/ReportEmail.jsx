// Email a report, list or register as a PDF (report_pdf.rs), through the company's email server
// (Settings › Email) when it is set up, or as a new message in the user's mail app.
import { useEffect, useRef, useState } from 'react';
import { api, errorText } from '../lib/api.js';

/** The attachment's name as the backend writes it (`forms::file_name`): unsafe characters become -. */
export function pdfFileName(title) {
  // eslint-disable-next-line no-control-regex
  const clean = String(title).replace(/[/\\:*?"<>|\u0000-\u001f\u007f]/g, '-').trim().replace(/^\.+|\.+$/g, '');
  return `${clean}.pdf`;
}

function composedText(outcome, fileName) {
  return outcome.kind === 'attached'
    ? `A new message is open in ${outcome.app} with ${fileName} attached. Review it there and send.`
    : `Your mail app opened a new message. Attach ${fileName}, shown in your file manager, then send.`;
}

/** `sheet` is the report as its Excel export builds it. */
export default function ReportEmailDialog({ sheet, onClose, onNotice }) {
  const fileName = pdfFileName(sheet.title);
  const [draft, setDraft] = useState({ to: '', subject: [sheet.title, sheet.subtitle].filter(Boolean).join(' · '), body: `Attached: ${sheet.title}${sheet.subtitle ? ` (${sheet.subtitle})` : ''}.\n\n${sheet.company ?? ''}`.trim() });
  const [smtp, setSmtp] = useState(null);
  const [method, setMethod] = useState('MAIL_APP');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  // Password-protect the PDF: it opens only with this password, which is not saved anywhere.
  const [protect, setProtect] = useState(false);
  const [password, setPassword] = useState('');
  const toRef = useRef(null);
  useEffect(() => {
    toRef.current?.focus();
    api
      .getSmtpSettings()
      .then((status) => {
        if (status.settings.enabled) {
          setSmtp({ fromEmail: status.settings.fromEmail });
          setMethod('SMTP');
        }
      })
      .catch(() => {});
  }, []);
  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && !busy && onClose();
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [busy, onClose]);
  const set = (key) => (e) => setDraft((d) => ({ ...d, [key]: e.target.value }));
  const send = async () => {
    setBusy(true);
    setError(null);
    const pdfPassword = protect ? password : null;
    try {
      if (method === 'SMTP') {
        await api.emailReportWithSmtp(sheet, draft, pdfPassword);
        onNotice({ kind: 'success', text: `Sent ${fileName} to ${draft.to}.` });
      } else {
        onNotice({ kind: 'success', text: composedText(await api.emailReportWithMailApp(sheet, draft, pdfPassword), fileName) });
      }
      onClose();
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-qb-nav/40 backdrop-blur-[2px]">
      <div role="dialog" aria-modal="true" aria-labelledby="email-report-title" className="w-[560px] rounded-xs bg-white shadow-popover">
        <div className="border-b border-qb-border px-3 py-1.5">
          <h2 id="email-report-title" className="text-md font-semibold">
            Email {sheet.title} as PDF
          </h2>
        </div>
        <div className="flex flex-col gap-1.5 px-3 py-2 text-sm">
          <label className="grid grid-cols-[64px_minmax(0,1fr)] items-center gap-2">
            <span className="text-xs font-semibold text-qb-ink-muted">To</span>
            <input ref={toRef} className="qb-input w-full" value={draft.to} onChange={set('to')} placeholder="accountant@example.com, another@example.com" />
          </label>
          <label className="grid grid-cols-[64px_minmax(0,1fr)] items-center gap-2">
            <span className="text-xs font-semibold text-qb-ink-muted">Subject</span>
            <input className="qb-input w-full" value={draft.subject} onChange={set('subject')} maxLength={200} />
          </label>
          <textarea aria-label="Message" rows={7} className="qb-input h-auto w-full py-0.5" value={draft.body} onChange={set('body')} />
          <div className="flex items-center gap-1.5 text-xs text-qb-ink-muted">
            <span className="rounded-xs border border-qb-border bg-qb-canvas px-1.5 py-0.5 font-semibold text-qb-ink">📎 {fileName}</span>
            <span>{method === 'SMTP' ? `sent from ${smtp.fromEmail}` : 'opens in your mail app as a new message for you to review and send'}</span>
          </div>
          <div className="flex items-center gap-2 text-xs">
            <label className="flex items-center gap-1">
              <input type="checkbox" checked={protect} onChange={(e) => setProtect(e.target.checked)} />
              Password-protect the PDF
            </label>
            {protect && (
              <input
                type="password"
                className="qb-input h-row w-[200px]"
                aria-label="PDF password"
                placeholder="Password to open it"
                autoComplete="new-password"
                maxLength={127}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            )}
          </div>
          {protect && <p className="text-2xs text-qb-ink-muted">Tell the recipient the password another way (by phone or text), not in this email. LocalBooks doesn’t keep it.</p>}
          {smtp && (
            <div className="flex gap-3 text-xs" role="radiogroup" aria-label="How to send">
              <label className="flex items-center gap-1">
                <input type="radio" name="report-send-method" checked={method === 'SMTP'} onChange={() => setMethod('SMTP')} />
                Send from LocalBooks
              </label>
              <label className="flex items-center gap-1">
                <input type="radio" name="report-send-method" checked={method === 'MAIL_APP'} onChange={() => setMethod('MAIL_APP')} />
                Open in my mail app instead
              </label>
            </div>
          )}
          {error && <p className="font-semibold text-qb-negative">{error}</p>}
        </div>
        <div className="flex justify-end gap-2 border-t border-qb-border px-3 py-1.5">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={send} disabled={busy || !draft.to.trim() || (protect && !password)}>
            {method === 'SMTP' ? (busy ? 'Sending…' : 'Send') : busy ? 'Opening…' : 'Open in mail app'}
          </button>
        </div>
      </div>
    </div>
  );
}
