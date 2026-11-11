import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';
import { askConfirm } from '../lib/dialogs.jsx';
import { codeReady, POLICIES, qrImageSource } from '../lib/twoStep.js';

/**
 * Two-step sign-in (gap R26). Optional and off by default: an Admin turns it on for the company,
 * and each user may then add an authenticator app. What it protects is said plainly on screen: it
 * stops someone with a password but not the phone from signing in to LocalBooks; it doesn't stop
 * someone who copies the company file.
 */

/** The code step of signing in, after a right password. */
export function CodeStep({ username, busy, run, onBack }) {
  const [code, setCode] = useState('');
  const [backup, setBackup] = useState(false);
  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        run(() => api.finishTwoStepSignIn(code));
      }}
    >
      <p className="text-sm text-qb-ink-muted">
        {username ? (
          <>
            <span className="font-semibold text-qb-ink">{username}</span> uses two-step sign-in.{' '}
          </>
        ) : null}
        {backup ? 'Enter one of the backup codes you saved when you set it up. Each works once.' : 'Enter the 6-digit code your authenticator app shows for LocalBooks.'}
      </p>
      <label className="block">
        <span className="qb-label">{backup ? 'Backup code' : 'Code'}</span>
        <input
          className="qb-input w-full font-mono tracking-wider"
          inputMode={backup ? 'text' : 'numeric'}
          autoComplete="one-time-code"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          autoFocus
          spellCheck={false}
          disabled={busy}
        />
      </label>
      <div className="flex items-center justify-between">
        <span className="flex gap-3 text-sm">
          <button type="button" className="text-qb-link hover:underline" onClick={onBack}>
            Back
          </button>
          <button type="button" className="text-qb-link hover:underline" onClick={() => (setBackup((b) => !b), setCode(''))}>
            {backup ? 'Use the app instead' : 'Use a backup code'}
          </button>
        </span>
        <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !codeReady(code)}>
          {busy ? 'Checking…' : 'Sign In'}
        </button>
      </div>
      <p className="text-2xs text-qb-ink-faint">
        Lost your phone and your backup codes? Another Admin can reset your two-step sign-in. The only Admin can use the recovery key under “I forgot my password”.
      </p>
    </form>
  );
}

/** Ten backup codes, shown once, with the same "I have saved them" step as the recovery key. */
function BackupCodes({ codes, onDone }) {
  const [saved, setSaved] = useState(false);
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(codes.join('\n'));
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div className="flex flex-col gap-2 text-sm">
      <p>
        Two-step sign-in is on for you. Save these backup codes somewhere apart from your phone: each one signs you in once if your phone is lost. LocalBooks shows them
        only now.
      </p>
      <ol data-testid="backup-codes" className="grid grid-cols-2 gap-1 rounded-xs border border-qb-border-strong bg-qb-canvas p-2 font-mono tracking-wider">
        {codes.map((c) => (
          <li key={c} className="select-all text-center">
            {c}
          </li>
        ))}
      </ol>
      <div className="flex items-center justify-between">
        <label className="flex items-center gap-2">
          <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />I have saved the backup codes
        </label>
        <span className="flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={copy}>
            {copied ? '✓ Copied' : 'Copy'}
          </button>
          <button type="button" className="qb-btn qb-btn-primary" disabled={!saved} onClick={onDone}>
            Done
          </button>
        </span>
      </div>
    </div>
  );
}

/** Setting up an authenticator app: password again, the QR code, the first code, backup codes. */
export function TwoStepSetup({ onDone, onCancel, required = false }) {
  const [password, setPassword] = useState('');
  const [view, setView] = useState(null);
  const [code, setCode] = useState('');
  const [codes, setCodes] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  const run = async (action) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  if (codes) return <BackupCodes codes={codes} onDone={onDone} />;

  return (
    <div className="flex flex-col gap-2 text-sm">
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      {!view ? (
        <form
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => {
              setView(await api.beginTwoStepSetup(password));
              setPassword('');
            });
          }}
        >
          <p>
            {required ? 'This company requires two-step sign-in for you. ' : ''}You need an authenticator app on your phone (any app that scans a QR code for a 6-digit
            code). Enter your password to begin.
          </p>
          <label className="block">
            <span className="qb-label">Your password</span>
            <input type="password" className="qb-input w-full" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} autoFocus disabled={busy} />
          </label>
          <div className="flex justify-end gap-2">
            {onCancel && (
              <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
                Cancel
              </button>
            )}
            <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !password}>
              {busy ? 'Checking…' : 'Continue'}
            </button>
          </div>
        </form>
      ) : (
        <form
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => setCodes(await api.finishTwoStepSetup(code)));
          }}
        >
          <p>Scan this QR code with your authenticator app. LocalBooks drew it on this computer; nothing was sent anywhere.</p>
          <img src={qrImageSource(view.qrSvg)} alt="QR code to scan into your authenticator app" className="mx-auto h-[200px] w-[200px] bg-white" />
          <p className="text-xs text-qb-ink-muted">
            Can’t scan? Type this key into the app instead (time-based):{' '}
            <code data-testid="two-step-key" className="select-all font-mono tracking-wider text-qb-ink">
              {view.secretText}
            </code>
          </p>
          <label className="block">
            <span className="qb-label">The 6-digit code the app now shows</span>
            <input
              className="qb-input w-full font-mono tracking-wider"
              inputMode="numeric"
              autoComplete="one-time-code"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              autoFocus
              disabled={busy}
            />
          </label>
          <div className="flex justify-end gap-2">
            {onCancel && (
              <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
                Cancel
              </button>
            )}
            <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !/^\d{6}$/.test(code.replace(/\s/g, ''))}>
              {busy ? 'Checking…' : 'Turn On'}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}

const WHAT_IT_PROTECTS =
  'Two-step sign-in stops someone who has a password but not the phone from signing in to LocalBooks. It does not stop someone who copies the company file: keep full-disk encryption (or whole-file encryption) on.';

/** Settings › Users & Passwords › Two-step sign-in: your own, and the company setting for an Admin. */
export function TwoStepSettings({ me }) {
  const [mine, setMine] = useState(null);
  const [mode, setMode] = useState(null); // 'setup' | 'remove'
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const [notice, setNotice] = useState(null);

  const load = () => api.getMyTwoStep().then(setMine, (err) => setError(errorText(err)));
  useEffect(() => {
    load();
  }, []);

  const run = async (action) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const setPolicy = (policy) =>
    run(async () => {
      if (policy === 'OFF' && !(await askConfirm('Turn two-step sign-in off? Everyone’s authenticator and backup codes are removed, and sign-in asks only for a password.', { confirmLabel: 'Turn Off' }))) return;
      await api.setTwoStepPolicy(policy);
      // A stricter setting may hold you at the set-up step; every window reloads.
      setNotice(policy === 'OFF' ? 'Two-step sign-in is off.' : 'Saved.');
      await load();
    });

  if (!mine) return error ? <Notice kind="error" text={error} /> : null;
  const isAdmin = me.role === 'ADMIN';
  if (mine.policy === 'OFF' && !isAdmin) return null;

  return (
    <section className="qb-panel">
      <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Two-step sign-in</h2>
      <div className="flex max-w-[600px] flex-col gap-2 p-2 text-sm">
        <p className="text-xs text-qb-ink-muted">{WHAT_IT_PROTECTS}</p>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        {notice && <Notice kind="success" text={notice} onDismiss={() => setNotice(null)} />}
        {isAdmin && (
          <fieldset className="flex flex-col gap-1" disabled={busy}>
            <legend className="qb-label">For this company</legend>
            {POLICIES.map(([id, label, hint]) => (
              <label key={id} className="flex items-baseline gap-2">
                <input type="radio" name="two-step-policy" checked={mine.policy === id} onChange={() => setPolicy(id)} />
                <span className="font-semibold">{label}</span>
                <span className="text-xs text-qb-ink-muted">{hint}</span>
              </label>
            ))}
          </fieldset>
        )}
        {mine.policy !== 'OFF' && mode === null && (
          <div className="flex items-center gap-2">
            <span>
              {mine.enabled
                ? `You sign in with a code from your authenticator app. ${mine.backupCodesLeft} backup code${mine.backupCodesLeft === 1 ? '' : 's'} left.`
                : 'You sign in with your password only.'}
            </span>
            <button type="button" className="qb-btn qb-btn-secondary h-row flex-none px-2 text-xs" onClick={() => setMode('setup')}>
              {mine.enabled ? 'Set Up Again…' : 'Set Up…'}
            </button>
            {mine.enabled && !mine.required && (
              <button type="button" className="qb-btn qb-btn-secondary h-row flex-none px-2 text-xs" onClick={() => setMode('remove')}>
                Turn Off…
              </button>
            )}
          </div>
        )}
        {mode === 'setup' && (
          <TwoStepSetup
            onCancel={() => setMode(null)}
            onDone={() => {
              setMode(null);
              setNotice('Two-step sign-in is on for you.');
              load();
            }}
          />
        )}
        {mode === 'remove' && (
          <form
            className="flex flex-col gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              run(async () => {
                await api.removeMyTwoStep(password);
                setPassword('');
                setMode(null);
                setNotice('Two-step sign-in is off for you.');
                await load();
              });
            }}
          >
            <label className="block">
              <span className="qb-label">Your password, to turn two-step sign-in off</span>
              <input type="password" className="qb-input w-full" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} autoFocus disabled={busy} />
            </label>
            <div className="flex justify-end gap-2">
              <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setMode(null)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !password}>
                Turn Off
              </button>
            </div>
          </form>
        )}
      </div>
    </section>
  );
}
