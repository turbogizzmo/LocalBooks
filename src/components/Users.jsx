import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import Notice from './Notice.jsx';
import { useCompanyFileActions } from './CompanyFiles.jsx';
import { api, COMPANY_FILE_CHANGED, errorText } from '../lib/api.js';
import { formatDate } from '../lib/dates.js';

const MIN_PASSWORD = 8;
const REMOVE_ALL = 'REMOVE ALL USERS';
const ROLES = [
  ['ADMIN', 'Admin', 'Everything, including users and passwords'],
  ['STANDARD', 'Standard', 'Everything except users and company-wide settings'],
  ['VIEW_ONLY', 'View only', 'Reads the books; changes nothing'],
];
const roleLabel = (role) => ROLES.find(([id]) => id === role)?.[1] ?? role;

const ACTIVITY_EVENTS = ['keydown', 'mousedown', 'mousemove', 'wheel', 'touchstart'];

/**
 * Signs out after the company's idle limit. Every 30 seconds a window reports whether anyone typed
 * or clicked in it; a quiet window asks the backend, which counts every window, whether the limit
 * has passed. The backend refuses commands past the limit on its own too (a Mac that slept).
 */
export function useIdleLock(state) {
  const minutes = state?.usersEnabled && state.user ? state.autoLockMinutes : 0;
  useEffect(() => {
    if (!minutes) return undefined;
    let active = false;
    const mark = () => {
      active = true;
    };
    ACTIVITY_EVENTS.forEach((e) => window.addEventListener(e, mark, { passive: true, capture: true }));
    const timer = setInterval(() => {
      // Signing out reloads every window (company-file-changed).
      if (active) api.touchSession().catch(() => {});
      else api.signOutIfIdle().catch(() => {});
      active = false;
    }, 30_000);
    return () => {
      clearInterval(timer);
      ACTIVITY_EVENTS.forEach((e) => window.removeEventListener(e, mark, { capture: true }));
    };
  }, [minutes]);
}

/** Reloads the window when another window signs in or out, or another company is opened. */
function useReloadOnCompanyChange() {
  useEffect(() => {
    let unlisten;
    let cancelled = false;
    listen(COMPANY_FILE_CHANGED, () => window.location.reload())
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}

function Field({ label, children, hint }) {
  return (
    <label className="block">
      <span className="qb-label">{label}</span>
      {children}
      {hint && <span className="block text-2xs text-qb-ink-faint">{hint}</span>}
    </label>
  );
}

/** A new password typed twice. `value` is `{ password, confirm }`. */
function NewPasswordFields({ value, onChange, disabled, label = 'New password' }) {
  const mismatch = value.confirm && value.password !== value.confirm;
  return (
    <>
      <Field label={label} hint={`At least ${MIN_PASSWORD} characters. A short phrase is easier to remember than a code.`}>
        <input
          type="password"
          className="qb-input w-full"
          autoComplete="new-password"
          value={value.password}
          onChange={(e) => onChange({ ...value, password: e.target.value })}
          disabled={disabled}
        />
      </Field>
      <Field label="Type it again">
        <input
          type="password"
          className="qb-input w-full"
          autoComplete="new-password"
          value={value.confirm}
          onChange={(e) => onChange({ ...value, confirm: e.target.value })}
          aria-invalid={mismatch}
          disabled={disabled}
        />
        {mismatch && <span className="block text-xs text-qb-negative">The passwords do not match.</span>}
      </Field>
    </>
  );
}

const EMPTY_PASSWORD = { password: '', confirm: '' };
const passwordReady = (p) => p.password.length >= MIN_PASSWORD && p.password === p.confirm;

/** A one-time view of a recovery key, with what it is for. */
export function RecoveryKeyPanel({ recoveryKey, onDone, doneLabel = 'Continue' }) {
  const [copied, setCopied] = useState(false);
  const [saved, setSaved] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(recoveryKey);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div className="flex flex-col gap-2">
      <p className="text-sm">
        This recovery key resets an Admin password if every Admin forgets theirs. LocalBooks does not keep a copy, and nobody else can reset it for you. Write it down or store
        it in a password manager, somewhere apart from this computer.
      </p>
      <div className="flex items-center gap-2">
        <code className="flex-1 select-all rounded-xs border border-qb-border-strong bg-qb-canvas px-2 py-1 text-center font-mono text-md tracking-wider">{recoveryKey}</code>
        <button type="button" className="qb-btn qb-btn-secondary flex-none" onClick={copy}>
          {copied ? '✓ Copied' : 'Copy'}
        </button>
      </div>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />I have saved the recovery key somewhere safe
      </label>
      <div className="flex justify-end">
        <button type="button" className="qb-btn qb-btn-primary" disabled={!saved} onClick={onDone}>
          {doneLabel}
        </button>
      </div>
    </div>
  );
}

function SignInForm({ busy, run, onForgot }) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [remember, setRemember] = useState(false);
  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        run(() => api.signIn(username, password, remember));
      }}
    >
      <Field label="User name">
        <input className="qb-input w-full" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} autoFocus disabled={busy} />
      </Field>
      <Field label="Password">
        <input type="password" className="qb-input w-full" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} disabled={busy} />
      </Field>
      <label className="flex items-start gap-2 text-sm">
        <input type="checkbox" className="mt-0.5" checked={remember} onChange={(e) => setRemember(e.target.checked)} disabled={busy} />
        <span>
          Remember me on this Mac
          <span className="block text-2xs text-qb-ink-faint">
            Opens this company as you without asking. Anyone who can use this Mac account could open your books, so leave it off on a shared computer.
          </span>
        </span>
      </label>
      <div className="flex items-center justify-between">
        <button type="button" className="text-sm text-qb-link hover:underline" onClick={onForgot}>
          I forgot my password
        </button>
        <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !username.trim() || !password}>
          {busy ? 'Signing in…' : 'Sign In'}
        </button>
      </div>
    </form>
  );
}

function ForgotChoices({ onRecover, onRemoveAll, onBack }) {
  return (
    <div className="flex flex-col gap-2 text-sm">
      <div className="rounded-xs border border-qb-border p-2">
        <div className="font-semibold">You are not an Admin</div>
        <p className="text-qb-ink-muted">Ask an Admin to set a temporary password for you in Settings › Users &amp; Passwords. You choose your own when you next sign in.</p>
      </div>
      <div className="rounded-xs border border-qb-border p-2">
        <div className="font-semibold">You are an Admin</div>
        <p className="mb-1 text-qb-ink-muted">Another Admin can reset your password the same way. Otherwise, use the recovery key you saved when users were turned on.</p>
        <button type="button" className="qb-btn qb-btn-secondary" onClick={onRecover}>
          Use the recovery key
        </button>
      </div>
      <div className="flex items-center justify-between">
        <button type="button" className="text-qb-link hover:underline" onClick={onBack}>
          Back to sign in
        </button>
        <button type="button" className="text-xs text-qb-ink-muted hover:underline" onClick={onRemoveAll}>
          No Admin password and no recovery key?
        </button>
      </div>
    </div>
  );
}

function RecoverForm({ busy, run, onBack }) {
  const [recoveryKey, setRecoveryKey] = useState('');
  const [username, setUsername] = useState('');
  const [next, setNext] = useState(EMPTY_PASSWORD);
  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        run(() => api.recoverWithRecoveryKey(recoveryKey, username, next.password));
      }}
    >
      <Field label="Recovery key" hint="Eight groups of four letters and digits. Dashes and capitals do not matter.">
        <input className="qb-input w-full font-mono" value={recoveryKey} onChange={(e) => setRecoveryKey(e.target.value)} autoFocus disabled={busy} spellCheck={false} />
      </Field>
      <Field label="Admin user name">
        <input className="qb-input w-full" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} disabled={busy} />
      </Field>
      <NewPasswordFields value={next} onChange={setNext} disabled={busy} />
      <div className="flex items-center justify-between">
        <button type="button" className="text-sm text-qb-link hover:underline" onClick={onBack}>
          Back
        </button>
        <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !recoveryKey.trim() || !username.trim() || !passwordReady(next)}>
          {busy ? 'Resetting…' : 'Reset Password and Sign In'}
        </button>
      </div>
    </form>
  );
}

function RemoveAllForm({ busy, run, onBack }) {
  const [typed, setTyped] = useState('');
  return (
    <form
      className="flex flex-col gap-2 text-sm"
      onSubmit={(e) => {
        e.preventDefault();
        run(() => api.removeAllUsers(typed));
      }}
    >
      <p>
        Without an Admin password or the recovery key, sensitive data such as vendor tax IDs cannot be unlocked by anyone. You can still keep the books: removing all users
        opens this company file without signing in, as before users were turned on.
      </p>
      <p className="font-semibold text-qb-negative">Every user is removed and every protected value is erased for good. Transactions, lists and reports are kept.</p>
      <Field label={`Type ${REMOVE_ALL} to confirm`}>
        <input className="qb-input w-full" value={typed} onChange={(e) => setTyped(e.target.value)} autoFocus disabled={busy} />
      </Field>
      <div className="flex items-center justify-between">
        <button type="button" className="text-qb-link hover:underline" onClick={onBack}>
          Back
        </button>
        <button type="submit" className="qb-btn qb-btn-primary bg-qb-negative" disabled={busy || typed.trim() !== REMOVE_ALL}>
          {busy ? 'Removing…' : 'Remove All Users'}
        </button>
      </div>
    </form>
  );
}

function ChangeOwnPasswordForm({ busy, run, temporary, onDone, submitLabel = 'Change Password' }) {
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState(EMPTY_PASSWORD);
  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        run(async () => {
          await api.changeMyPassword(current, next.password);
          setCurrent('');
          setNext(EMPTY_PASSWORD);
          onDone?.();
        });
      }}
    >
      <Field label={temporary ? 'Temporary password' : 'Current password'}>
        <input type="password" className="qb-input w-full" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} autoFocus={temporary} disabled={busy} />
      </Field>
      <NewPasswordFields value={next} onChange={setNext} disabled={busy} />
      <div className="flex justify-end">
        <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !current || !passwordReady(next)}>
          {busy ? 'Saving…' : submitLabel}
        </button>
      </div>
    </form>
  );
}

/**
 * Shown instead of the app when the open company file has users and nobody is signed in, or when
 * the signed-in user still has a temporary password.
 */
export default function SignIn({ companyName, user }) {
  const [mode, setMode] = useState(user?.mustChangePassword ? 'change' : 'signin');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const actions = useCompanyFileActions();
  useReloadOnCompanyChange();

  // Signing in reloads every window (the backend says so); errors stay here.
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
  const go = (next) => {
    setError(null);
    setMode(next);
  };

  const titles = {
    signin: 'Sign in',
    forgot: 'Forgot your password?',
    recover: 'Reset an Admin password',
    removeAll: 'Remove all users',
    change: 'Choose your own password',
  };

  return (
    <div className="flex h-full items-center justify-center bg-qb-canvas p-4">
      <section className="qb-panel flex w-full max-w-[440px] flex-col gap-2 p-3" aria-labelledby="sign-in-title">
        <div>
          <div className="text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">{companyName ?? 'Company file'}</div>
          <h1 id="sign-in-title" className="text-[24px] font-light leading-tight">
            {titles[mode]}
          </h1>
        </div>
        {(error ?? actions.error) && <Notice kind="error" text={error ?? actions.error} onDismiss={() => (setError(null), actions.clearError())} />}
        {mode === 'signin' && <SignInForm busy={busy} run={run} onForgot={() => go('forgot')} />}
        {mode === 'forgot' && <ForgotChoices onRecover={() => go('recover')} onRemoveAll={() => go('removeAll')} onBack={() => go('signin')} />}
        {mode === 'recover' && <RecoverForm busy={busy} run={run} onBack={() => go('forgot')} />}
        {mode === 'removeAll' && <RemoveAllForm busy={busy} run={run} onBack={() => go('forgot')} />}
        {mode === 'change' && (
          <>
            <p className="text-sm text-qb-ink-muted">
              Signed in as <span className="font-semibold text-qb-ink">{user?.displayName}</span>. An Admin set your password; choose one only you know.
            </p>
            <ChangeOwnPasswordForm busy={busy} run={run} temporary submitLabel="Save and Continue" onDone={() => window.location.reload()} />
          </>
        )}
        <div className="flex gap-3 border-t border-qb-border pt-2 text-xs">
          {mode === 'change' ? (
            <button type="button" className="text-qb-link hover:underline" onClick={() => run(api.signOut)}>
              Sign out
            </button>
          ) : (
            <>
              <button type="button" className="text-qb-link hover:underline" onClick={actions.openOrRestore}>
                Open another company
              </button>
              <button type="button" className="text-qb-link hover:underline" onClick={() => run(api.closeCompanyFile)}>
                Close company
              </button>
            </>
          )}
        </div>
      </section>
      {actions.dialog}
    </div>
  );
}

function TurnOnUsers({ onTurnedOn }) {
  const [username, setUsername] = useState('Admin');
  const [displayName, setDisplayName] = useState('');
  const [next, setNext] = useState(EMPTY_PASSWORD);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  const submit = async (e) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      onTurnedOn(await api.turnOnUsers(username, displayName, next.password));
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <form className="flex max-w-[560px] flex-col gap-2 p-2" onSubmit={submit}>
      <p className="text-sm">
        Right now anyone who opens this company file can see and change everything. With users turned on, everyone signs in with their own password, and sensitive data
        such as vendor tax IDs is encrypted so only users you allow can read it.
      </p>
      <p className="text-sm text-qb-ink-muted">You become the first Admin. Afterwards you get a recovery key, the only way back in if every Admin forgets their password.</p>
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      <div className="grid grid-cols-2 gap-2">
        <Field label="Your user name">
          <input className="qb-input w-full" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} disabled={busy} />
        </Field>
        <Field label="Your name (optional)">
          <input className="qb-input w-full" value={displayName} onChange={(e) => setDisplayName(e.target.value)} disabled={busy} />
        </Field>
      </div>
      <NewPasswordFields value={next} onChange={setNext} disabled={busy} label="Your password" />
      <div className="flex justify-end">
        <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !username.trim() || !passwordReady(next)}>
          {busy ? 'Turning on…' : 'Turn On Users'}
        </button>
      </div>
    </form>
  );
}

/** Add a user, or edit one (`user` given): role, sensitive data, on or off, and a password reset. */
function UserDialog({ user, me, onClose, onSaved }) {
  const editing = Boolean(user);
  const self = editing && user.id === me.id;
  const [username, setUsername] = useState(user?.username ?? '');
  const [displayName, setDisplayName] = useState(user?.displayName ?? '');
  const [role, setRole] = useState(user?.role ?? 'STANDARD');
  const [sensitive, setSensitive] = useState(user?.canSeeSensitive ?? false);
  const [active, setActive] = useState(user?.isActive ?? true);
  const [resetting, setResetting] = useState(!editing);
  const [next, setNext] = useState(EMPTY_PASSWORD);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && !busy && onClose();
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [busy, onClose]);

  const isAdmin = role === 'ADMIN';
  // Granting sensitive data (or turning a user with it back on) needs a password to store the key for.
  const needsPassword = !editing || ((isAdmin || sensitive) && active && (!user.canSeeSensitive || !user.isActive));
  const showPassword = resetting || needsPassword;
  const ready = (editing || username.trim()) && (!showPassword || passwordReady(next));

  const save = async (e) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const password = showPassword ? next.password : null;
      const saved = editing
        ? await api.updateUser(user.id, { displayName, role, canSeeSensitive: isAdmin || sensitive, isActive: active, newPassword: password })
        : await api.addUser({ username, displayName: displayName || null, role, canSeeSensitive: isAdmin || sensitive, password });
      onSaved(saved);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/30" role="dialog" aria-modal="true" aria-label={editing ? `Edit ${user.username}` : 'Add a user'}>
      <form className="qb-panel flex w-[480px] flex-col gap-2 p-3 shadow-panel" onSubmit={save}>
        <h2 className="text-md font-semibold">{editing ? `Edit ${user.username}` : 'Add a user'}</h2>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="grid grid-cols-2 gap-2">
          <Field label="User name">
            <input className="qb-input w-full" value={username} onChange={(e) => setUsername(e.target.value)} disabled={busy || editing} autoFocus={!editing} />
          </Field>
          <Field label="Name (optional)">
            <input className="qb-input w-full" value={displayName} onChange={(e) => setDisplayName(e.target.value)} disabled={busy} />
          </Field>
        </div>
        <fieldset className="flex flex-col gap-1" disabled={busy || self}>
          <legend className="qb-label">Role{self && ' (another Admin can change yours)'}</legend>
          {ROLES.map(([id, label, hint]) => (
            <label key={id} className="flex items-baseline gap-2 text-sm">
              <input type="radio" name="role" checked={role === id} onChange={() => setRole(id)} />
              <span className="font-semibold">{label}</span>
              <span className="text-xs text-qb-ink-muted">{hint}</span>
            </label>
          ))}
        </fieldset>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={isAdmin || sensitive} disabled={busy || isAdmin} onChange={(e) => setSensitive(e.target.checked)} />
          Can see sensitive data (vendor tax IDs){isAdmin && <span className="text-xs text-qb-ink-muted">: Admins always can</span>}
        </label>
        {editing && !self && (
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={active} disabled={busy} onChange={(e) => setActive(e.target.checked)} />
            User is turned on (can sign in)
          </label>
        )}
        {editing && !needsPassword && (
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={resetting} disabled={busy} onChange={(e) => setResetting(e.target.checked)} />
            Set a new temporary password
          </label>
        )}
        {showPassword && (
          <>
            {editing && needsPassword && (
              <p className="text-xs text-qb-ink-muted">Sensitive data can only be stored for a password, so set a temporary one for this user.</p>
            )}
            <NewPasswordFields value={next} onChange={setNext} disabled={busy} label="Temporary password" />
            <p className="text-xs text-qb-ink-muted">Tell the user this password; they choose their own when they next sign in.</p>
          </>
        )}
        <div className="flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !ready}>
            {busy ? 'Saving…' : editing ? 'Save' : 'Add User'}
          </button>
        </div>
      </form>
    </div>
  );
}

function UserList({ me }) {
  const [users, setUsers] = useState(null);
  const [error, setError] = useState(null);
  const [editing, setEditing] = useState(null); // a user, or 'new'
  const [recoveryKey, setRecoveryKey] = useState(null);
  const [notice, setNotice] = useState(null);

  const load = useCallback(() => api.listUsers().then(setUsers, (err) => setError(errorText(err))), []);
  useEffect(() => {
    load();
  }, [load]);

  const replaceKey = async () => {
    if (!window.confirm('Issue a new recovery key? The one you saved stops working.')) return;
    try {
      setRecoveryKey(await api.replaceRecoveryKey());
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <section className="qb-panel">
      <h2 className="flex items-center border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
        Users
        <span className="ml-auto flex gap-2 normal-case tracking-normal">
          <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs" onClick={replaceKey}>
            New Recovery Key…
          </button>
          <button type="button" className="qb-btn qb-btn-primary h-row px-2 text-xs" onClick={() => setEditing('new')}>
            Add User…
          </button>
        </span>
      </h2>
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      {notice && <Notice kind="success" text={notice} onDismiss={() => setNotice(null)} />}
      {recoveryKey && (
        <div className="border-b border-qb-border p-2">
          <RecoveryKeyPanel recoveryKey={recoveryKey} doneLabel="Done" onDone={() => setRecoveryKey(null)} />
        </div>
      )}
      <table className="qb-grid">
        <thead>
          <tr>
            <th className="qb-grid-head">User name</th>
            <th className="qb-grid-head">Name</th>
            <th className="qb-grid-head">Role</th>
            <th className="qb-grid-head">Sensitive data</th>
            <th className="qb-grid-head">Last signed in</th>
            <th className="qb-grid-head" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          {(users ?? []).map((u) => (
            <tr key={u.id} className={`qb-grid-row ${u.isActive ? '' : 'text-qb-ink-muted'}`}>
              <td className="qb-grid-cell">
                {u.username}
                {u.id === me.id && <span className="ml-1 text-xs text-qb-ink-muted">(you)</span>}
                {!u.isActive && <span className="ml-1 text-xs">(turned off)</span>}
                {u.isActive && u.mustChangePassword && <span className="ml-1 text-xs text-qb-ink-muted">(temporary password)</span>}
              </td>
              <td className="qb-grid-cell">{u.displayName}</td>
              <td className="qb-grid-cell">{roleLabel(u.role)}</td>
              <td className="qb-grid-cell">{u.canSeeSensitive ? 'Yes' : 'No'}</td>
              <td className="qb-grid-cell">{u.lastSignedInAt ? formatDate(u.lastSignedInAt.slice(0, 10)) : 'Never'}</td>
              <td className="qb-grid-cell cell-tight">
                <button type="button" className="qb-btn qb-btn-secondary h-[20px] px-2 text-xs" onClick={() => setEditing(u)}>
                  Edit…
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {editing && (
        <UserDialog
          user={editing === 'new' ? null : editing}
          me={me}
          onClose={() => setEditing(null)}
          onSaved={(saved) => {
            setEditing(null);
            setNotice(editing === 'new' ? `Added ${saved.username}. Give them their temporary password.` : `Saved ${saved.username}.`);
            load();
          }}
        />
      )}
    </section>
  );
}

/** Admin: how long a signed-in user may be idle before LocalBooks signs them out. */
function AutoLockSetting({ minutes }) {
  const [value, setValue] = useState(String(minutes));
  const [status, setStatus] = useState(null);
  const save = async (next) => {
    setValue(next);
    try {
      await api.setPreference('auto_lock_minutes', next);
      setStatus({ kind: 'success', text: next === '0' ? 'LocalBooks will not sign anyone out for being idle.' : `Signs out after ${next} minutes without activity.` });
    } catch (err) {
      setStatus({ kind: 'error', text: errorText(err) });
    }
  };
  return (
    <section className="qb-panel">
      <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Sign out when idle</h2>
      <div className="flex max-w-[560px] flex-col gap-1 p-2 text-sm">
        <label className="flex items-center gap-2">
          <span>Sign everyone out after</span>
          <select className="qb-input w-[140px]" value={value} onChange={(e) => save(e.target.value)}>
            {['5', '10', '15', '30', '60', '120', '0'].map((m) => (
              <option key={m} value={m}>
                {m === '0' ? 'Never' : `${m} minutes`}
              </option>
            ))}
            {!['5', '10', '15', '30', '60', '120', '0'].includes(value) && <option value={value}>{value} minutes</option>}
          </select>
          <span>without activity</span>
        </label>
        <span className="text-xs text-qb-ink-muted">Counts typing and clicking in every LocalBooks window, and time the Mac was asleep. Anything not yet saved is lost when it signs out.</span>
        {status && <Notice kind={status.kind} text={status.text} onDismiss={() => setStatus(null)} />}
      </div>
    </section>
  );
}

/** Settings › Users & Passwords. */
export function UsersSettings() {
  const [state, setState] = useState(null);
  const [error, setError] = useState(null);
  const [turnedOn, setTurnedOn] = useState(null);
  const [busy, setBusy] = useState(false);
  const [changed, setChanged] = useState(false);

  useEffect(() => {
    api.getSignInState().then(setState, (err) => setError(errorText(err)));
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

  if (error && !state) return <Notice kind="error" text={error} />;
  if (!state) return null;

  if (turnedOn) {
    return (
      <section className="qb-panel max-w-[600px] p-2">
        <h2 className="mb-1 text-md font-semibold">Users are on. Save your recovery key</h2>
        <p className="mb-2 text-sm text-qb-ink-muted">
          Tax IDs in this file are now encrypted. Backups made before today still contain them in plain text: delete the ones you no longer need.
        </p>
        {/* Reloading signs every window in as the new Admin. */}
        <RecoveryKeyPanel recoveryKey={turnedOn.recoveryKey} onDone={() => window.location.reload()} />
      </section>
    );
  }

  if (!state.usersEnabled) {
    return (
      <section className="qb-panel">
        <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Users &amp; Passwords</h2>
        <TurnOnUsers onTurnedOn={setTurnedOn} />
      </section>
    );
  }

  const me = state.user;
  return (
    <div className="flex flex-col gap-2 overflow-auto scrollbar-thin">
      <section className="qb-panel">
        <h2 className="flex items-center border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
          Your sign-in
          <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row px-2 text-xs normal-case tracking-normal" onClick={() => run(api.signOut)} disabled={busy}>
            Sign Out
          </button>
        </h2>
        <div className="flex max-w-[560px] flex-col gap-2 p-2">
          <p className="text-sm">
            Signed in as <span className="font-semibold">{me.displayName}</span> ({me.username}), {roleLabel(me.role)}.{' '}
            {me.canSeeSensitive ? 'You can see sensitive data.' : 'You cannot see sensitive data.'}
          </p>
          {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
          {changed && <Notice kind="success" text="Your password was changed." onDismiss={() => setChanged(false)} />}
          {state.rememberedHere && (
            <div className="flex items-center gap-2 text-sm">
              <span>This Mac remembers a sign-in for this company and opens it without asking.</span>
              <button
                type="button"
                className="qb-btn qb-btn-secondary h-row flex-none px-2 text-xs"
                disabled={busy}
                onClick={() => run(async () => (await api.forgetRememberedSignIn(), setState((s) => ({ ...s, rememberedHere: false }))))}
              >
                Forget on This Mac
              </button>
            </div>
          )}
          <ChangeOwnPasswordForm busy={busy} run={run} onDone={() => setChanged(true)} />
        </div>
      </section>
      {me.role === 'ADMIN' && <AutoLockSetting minutes={state.autoLockMinutes} />}
      {me.role === 'ADMIN' && <UserList me={me} />}
    </div>
  );
}
