import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';

const PORTS = { STARTTLS: 587, TLS: 465, NONE: 25 };

function Field({ label, htmlFor, hint, children }) {
  return (
    <div className="grid grid-cols-[170px_minmax(0,1fr)] items-center gap-2 border-b border-qb-border px-2 py-1 last:border-b-0">
      <label htmlFor={htmlFor} className="text-sm font-semibold text-qb-ink-muted">
        {label}
      </label>
      <div>
        {children}
        {hint && <div className="text-2xs text-qb-ink-faint">{hint}</div>}
      </div>
    </div>
  );
}

/**
 * Settings › Email: how emailed forms leave LocalBooks. By default they open in the user's mail
 * app; optionally they are sent directly through the company's SMTP server.
 */
export default function EmailSettings() {
  const [saved, setSaved] = useState(null);
  const [draft, setDraft] = useState(null);
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    api
      .getSmtpSettings()
      .then((status) => {
        setSaved(status);
        setDraft(status.settings);
      })
      .catch((err) => setNotice({ kind: 'error', text: errorText(err) }));
  }, []);

  if (!draft) return notice ? <Notice kind={notice.kind} text={notice.text} /> : <div className="px-2 py-1 text-sm text-qb-ink-muted">Loading…</div>;

  const set = (key) => (value) => setDraft((d) => ({ ...d, [key]: value }));
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved.settings) || password !== '';
  const needsPassword = draft.username.trim() !== '';

  const run = async (action, success) => {
    setBusy(true);
    setNotice(null);
    try {
      await action();
      setNotice({ kind: 'success', text: success });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };
  const save = () =>
    run(async () => {
      const status = await api.saveSmtpSettings(draft, password || null);
      setSaved(status);
      setDraft(status.settings);
      setPassword('');
    }, 'Email settings saved.');
  const test = () => run(() => api.testSmtpSettings(draft, password || null), `Connected to ${draft.host} and signed in.`);
  const sendTest = async () => {
    setBusy(true);
    setNotice(null);
    try {
      const to = await api.sendSmtpTestEmail(draft, password || null);
      setNotice({ kind: 'success', text: `Test email sent to ${to}. Check that inbox (and its spam folder).` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };
  const forgetPassword = () =>
    run(async () => {
      const status = await api.saveSmtpSettings(saved.settings, '');
      setSaved(status);
      setPassword('');
    }, 'Password removed from this computer.');

  return (
    <div className="flex max-w-[760px] flex-col gap-2">
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      <section className="qb-panel">
        <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Emailed forms</h2>
        <div className="flex flex-col gap-1 px-2 py-1.5 text-sm">
          <label className="flex items-start gap-2">
            <input type="radio" name="email-method" className="mt-1" checked={!draft.enabled} onChange={() => set('enabled')(false)} />
            <span>
              <span className="font-semibold">Open them in my mail app</span>
              <span className="block text-xs text-qb-ink-muted">A new message opens in Mail, Outlook or your default mail app with the PDF attached. You review it and send it there.</span>
            </span>
          </label>
          <label className="flex items-start gap-2">
            <input type="radio" name="email-method" className="mt-1" checked={draft.enabled} onChange={() => set('enabled')(true)} />
            <span>
              <span className="font-semibold">Send them from LocalBooks (SMTP)</span>
              <span className="block text-xs text-qb-ink-muted">
                LocalBooks sends the message through your email provider’s server. Use the SMTP settings your provider publishes; many need an app
                password rather than your usual one.
              </span>
            </span>
          </label>
        </div>
      </section>
      <section className="qb-panel">
        <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Email server (SMTP)</h2>
        <Field label="Server" htmlFor="smtp-host" hint="For example smtp.gmail.com, smtp.office365.com or smtp.fastmail.com">
          <input id="smtp-host" className="qb-input w-full" value={draft.host} onChange={(e) => set('host')(e.target.value)} placeholder="smtp.example.com" autoComplete="off" spellCheck={false} />
        </Field>
        <Field label="Security and port" htmlFor="smtp-security">
          <div className="flex gap-2">
            <select
              id="smtp-security"
              className="qb-input w-[260px]"
              value={draft.security}
              onChange={(e) => setDraft((d) => ({ ...d, security: e.target.value, port: PORTS[e.target.value] }))}
            >
              <option value="STARTTLS">STARTTLS (usually port 587)</option>
              <option value="TLS">SSL/TLS (usually port 465)</option>
              <option value="NONE">None: local relay without a password</option>
            </select>
            <input
              aria-label="Port"
              className="qb-input w-[90px]"
              inputMode="numeric"
              value={draft.port}
              onChange={(e) => set('port')(Number(e.target.value.replace(/\D/g, '').slice(0, 5)) || 0)}
            />
          </div>
        </Field>
        <Field label="Username" htmlFor="smtp-username" hint="Usually your full email address. Leave empty for a server that needs no sign-in.">
          <input id="smtp-username" className="qb-input w-full" value={draft.username} onChange={(e) => set('username')(e.target.value)} autoComplete="off" spellCheck={false} />
        </Field>
        {needsPassword && (
          <Field
            label="Password"
            htmlFor="smtp-password"
            hint={saved.hasPassword ? 'A password is saved on this computer. Type a new one to replace it.' : 'Kept in this computer’s Keychain or Credential Manager, never in the company file.'}
          >
            <div className="flex gap-2">
              <input
                id="smtp-password"
                type="password"
                className="qb-input w-full"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={saved.hasPassword ? '••••••••' : ''}
                autoComplete="new-password"
              />
              {saved.hasPassword && (
                <button type="button" className="qb-btn qb-btn-secondary h-row flex-none" disabled={busy} onClick={forgetPassword}>
                  Forget password
                </button>
              )}
            </div>
          </Field>
        )}
        <Field label="From address" htmlFor="smtp-from">
          <input id="smtp-from" type="email" className="qb-input w-full" value={draft.fromEmail} onChange={(e) => set('fromEmail')(e.target.value)} placeholder="billing@yourcompany.com" spellCheck={false} />
        </Field>
        <Field label="From name" htmlFor="smtp-from-name" hint="Shown to customers, for example your company name">
          <input id="smtp-from-name" className="qb-input w-full" value={draft.fromName} onChange={(e) => set('fromName')(e.target.value)} maxLength={100} />
        </Field>
        <Field label="Copy to me" htmlFor="smtp-copy">
          <label className="flex items-center gap-1.5 text-sm">
            <input id="smtp-copy" type="checkbox" checked={draft.copyToSelf} onChange={(e) => set('copyToSelf')(e.target.checked)} />
            Send a blind copy of every message to the from address
          </label>
        </Field>
      </section>
      <div className="flex justify-end gap-1">
        <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy || !draft.host.trim()} onClick={test}>
          Test connection
        </button>
        <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy || !draft.host.trim() || !draft.fromEmail.trim()} onClick={sendTest} title="Sends a short message to the from address">
          Send test email
        </button>
        <button type="button" className="qb-btn qb-btn-primary h-row" disabled={busy || !dirty} onClick={save}>
          Save
        </button>
      </div>
    </div>
  );
}
