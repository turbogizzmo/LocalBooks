import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { useFormPrinter } from './FormPages.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatMoney } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

const KIND_LABEL = { INVOICE: 'Invoice', ESTIMATE: 'Estimate', SALES_RECEIPT: 'Sales receipt' };
const key = (row) => `${row.kind}:${row.documentId}`;
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Sales › Send Forms: the forms marked "Email later". Each goes out with its form's email template
 * and PDF: all at once over SMTP when sending from LocalBooks is on, otherwise as one new message
 * per form in the mail app. A sent form leaves the queue and its send is added to its history.
 */
export default function SendForms() {
  const { data, error, reload } = useLedgerQuery(api.listEmailQueue);
  const [smtp, setSmtp] = useState(null);
  const [method, setMethod] = useState('MAIL_APP');
  const [selected, setSelected] = useState(() => new Set());
  const [addresses, setAddresses] = useState({});
  const [results, setResults] = useState({});
  const [progress, setProgress] = useState(null);
  const [notice, setNotice] = useState(null);
  const [printPortal, forms] = useFormPrinter(setNotice);
  const rows = useMemo(() => data ?? [], [data]);

  useEffect(() => {
    api
      .getSmtpSettings()
      .then((status) => {
        if (status.settings.enabled) {
          setSmtp(status.settings);
          setMethod('SMTP');
        }
      })
      .catch(() => {});
  }, []);
  // New rows start selected when they have an address; rows that left the queue drop out.
  useEffect(() => {
    setSelected((prev) => {
      const next = new Set();
      for (const row of rows) if (prev.has(key(row)) || (!prev.size && row.customerEmail)) next.add(key(row));
      return next;
    });
  }, [rows]);

  const addressOf = (row) => addresses[key(row)] ?? row.customerEmail ?? '';
  const chosen = rows.filter((row) => selected.has(key(row)));
  const busy = progress !== null;
  const toggle = (row) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key(row))) next.delete(key(row));
      else next.add(key(row));
      return next;
    });

  const send = async () => {
    setNotice(null);
    const outcome = {};
    let sent = 0;
    for (const [i, row] of chosen.entries()) {
      setProgress({ done: i, total: chosen.length });
      const to = addressOf(row).trim();
      if (!to) {
        outcome[key(row)] = { ok: false, text: 'No email address' };
        continue;
      }
      try {
        const draft = await api.getEmailDraft(row.kind, row.documentId);
        const email = { to, subject: draft.subject, body: draft.body };
        if (method === 'SMTP') {
          await api.emailFormWithSmtp(row.kind, row.documentId, email);
        } else {
          await api.emailFormWithMailApp(row.kind, row.documentId, email);
          // Give the mail app a moment to open each message before the next.
          await pause(600);
        }
        outcome[key(row)] = { ok: true, text: method === 'SMTP' ? 'Sent' : 'Opened in mail app' };
        sent += 1;
      } catch (err) {
        outcome[key(row)] = { ok: false, text: errorText(err) };
      }
    }
    setProgress(null);
    setResults(outcome);
    const failed = chosen.length - sent;
    const what = method === 'SMTP' ? `Sent ${sent} of ${chosen.length}` : `Opened ${sent} of ${chosen.length} in your mail app`;
    setNotice({ kind: failed ? 'error' : 'success', text: failed ? `${what}. ${failed} could not be sent: see the Result column.` : `${what}.` });
    reload();
  };

  const remove = async (row) => {
    await api.setEmailLater(row.kind, row.documentId, false);
    reload();
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printPortal}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load the queue: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-3 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Send Forms</h2>
          <span className="text-xs text-qb-ink-muted">Forms marked Email later, sent with each form’s email template and PDF</span>
          <div className="ml-auto flex items-center gap-3 text-xs">
            {smtp ? (
              <>
                <label className="flex items-center gap-1">
                  <input type="radio" name="send-forms-method" checked={method === 'SMTP'} onChange={() => setMethod('SMTP')} disabled={busy} />
                  Send from LocalBooks ({smtp.fromEmail})
                </label>
                <label className="flex items-center gap-1">
                  <input type="radio" name="send-forms-method" checked={method === 'MAIL_APP'} onChange={() => setMethod('MAIL_APP')} disabled={busy} />
                  Open one message per form in my mail app
                </label>
              </>
            ) : (
              <span className="text-qb-ink-muted">Opens one message per form in your mail app · to send them all at once, set up Settings › Email</span>
            )}
            <button type="button" className="qb-btn qb-btn-primary h-row" disabled={busy || chosen.length === 0} onClick={send}>
              {busy ? `Sending ${progress.done + 1} of ${progress.total}…` : `${method === 'SMTP' ? 'Send' : 'Open'} ${chosen.length || ''} selected`.replace('  ', ' ')}
            </button>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[34px]" />
              <col className="w-[92px]" />
              <col className="w-[72px]" />
              <col />
              <col className="w-[92px]" />
              <col className="w-[112px]" />
              <col className="w-[260px]" />
              <col className="w-[220px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head text-center">
                  <input
                    type="checkbox"
                    aria-label="Select all"
                    checked={rows.length > 0 && chosen.length === rows.length}
                    onChange={(e) => setSelected(e.target.checked ? new Set(rows.map(key)) : new Set())}
                    disabled={busy}
                  />
                </th>
                <th className="qb-grid-head">Type</th>
                <th className="qb-grid-head">No.</th>
                <th className="qb-grid-head">Customer</th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head text-right">Total</th>
                <th className="qb-grid-head">Email to</th>
                <th className="qb-grid-head">Result</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={9} className="qb-grid-cell text-qb-ink-muted">
                    Nothing to send. Tick Email later on an invoice or estimate, or choose Email later in a row’s menu, to queue it here.
                  </td>
                </tr>
              )}
              {rows.map((row) => {
                const result = results[key(row)];
                return (
                  <tr key={key(row)} className="qb-grid-row">
                    <td className="qb-grid-cell text-center">
                      <input type="checkbox" aria-label={`Select ${KIND_LABEL[row.kind]} ${row.number}`} checked={selected.has(key(row))} onChange={() => toggle(row)} disabled={busy} />
                    </td>
                    <td className="qb-grid-cell">{KIND_LABEL[row.kind]}</td>
                    <td className="qb-grid-cell">{row.number}</td>
                    <td className="qb-grid-cell font-semibold" title={row.customerName}>{row.customerName}</td>
                    <td className="qb-grid-cell">{formatDate(row.date)}</td>
                    <td className="qb-grid-cell num">{formatMoney(row.totalCents, row.currencyCode)}</td>
                    <td className="qb-grid-cell">
                      <input
                        className={`qb-input w-full ${addressOf(row) ? '' : 'border-qb-warning'}`}
                        aria-label={`Email address for ${KIND_LABEL[row.kind]} ${row.number}`}
                        value={addressOf(row)}
                        placeholder="No email address on file"
                        onChange={(e) => setAddresses((a) => ({ ...a, [key(row)]: e.target.value }))}
                        disabled={busy}
                      />
                    </td>
                    <td className={`qb-grid-cell text-xs ${result ? (result.ok ? 'text-qb-green-active' : 'text-qb-negative') : 'text-qb-ink-muted'}`} title={result?.text}>
                      {result?.text ?? ''}
                    </td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        items={[
                          { label: 'Review and email…', onSelect: () => forms.email(row.kind, row.documentId) },
                          { label: 'Print…', onSelect: () => forms.print(row.kind, row.documentId) },
                          { label: 'Remove from Email later', onSelect: () => remove(row) },
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
