import { forwardRef, useState } from 'react';
import { api, errorText, useLedgerQuery } from '../lib/api.js';

/** The company's classes, kept current as they change. */
export function useClasses() {
  return useLedgerQuery(api.listClasses).data ?? [];
}

/**
 * Class tracking as the company has set it up (Settings › Class tracking):
 * `enabled` turns the feature on, `prompt` warns before saving lines without a class, and
 * `perTransaction` assigns one class to a whole form instead of one per line.
 */
export function useClassTracking() {
  const classes = useClasses();
  const prefs = useLedgerQuery(api.getPreferences).data;
  const enabled = prefs?.class_tracking === 'true';
  return {
    classes,
    enabled,
    prompt: enabled && prefs?.class_prompt === 'true',
    perTransaction: enabled && prefs?.class_assign === 'transaction',
    hasClasses: classes.some((c) => c.isActive),
  };
}

/**
 * Whether a grid shows its Class column: tracking is on, per line, or a row already has a class.
 * With tracking on it shows even before the first class exists, since the picker can add one.
 */
export function showsClassColumn(tracking, rows) {
  return (tracking.enabled && !tracking.perTransaction) || rows.some((r) => r.classId != null);
}

const lineAccount = (line) => line.accountId ?? line.incomeAccountId ?? line.account_id ?? null;
const PL = new Set(['INCOME', 'EXPENSE']);

/**
 * Class handling for one entry form: the one-class-per-transaction picker, applying that class to
 * the lines on save, and the warning before posting income or expense lines without a class.
 */
export function useFormClass(accounts = []) {
  const tracking = useClassTracking();
  const [formClass, setFormClass] = useState(null);
  const [warnedFor, setWarnedFor] = useState(null);

  const header =
    tracking.perTransaction ? (
      <label>
        <span className="qb-label">Class</span>
        <ClassSelect className="qb-input w-[170px]" classes={tracking.classes} value={formClass} onChange={setFormClass} aria-label="Class for this transaction" />
      </label>
    ) : null;

  /** The lines as they should be posted: with the form's class on each when assigned per transaction. */
  const apply = (lines, key = 'classId') => (tracking.perTransaction ? lines.map((l) => ({ ...l, [key]: formClass })) : lines);

  /**
   * The first time income or expense lines without a class are saved, returns a warning to show
   * instead of posting; saving again the same way posts. Returns null when there is nothing to warn about.
   */
  const missing = (postedLines, key = 'classId') => {
    if (!tracking.prompt) return null;
    const relevant = postedLines.filter((l) => {
      const account = accounts.find((a) => a.id === lineAccount(l));
      return !account || PL.has(account.classification);
    });
    const count = relevant.filter((l) => l[key] == null).length;
    if (count === 0) return null;
    const signature = JSON.stringify(postedLines);
    if (warnedFor === signature) return null;
    setWarnedFor(signature);
    return `${count} income or expense line${count === 1 ? ' has' : 's have'} no class. Choose a class, or save again to post without one.`;
  };

  const reset = () => {
    setFormClass(null);
    setWarnedFor(null);
  };

  return { ...tracking, formClass, setFormClass, header, apply, missing, reset };
}

export function classLabel(classes, id) {
  return classes.find((c) => c.id === id)?.fullName ?? '';
}

const ADD_NEW = '__new';

/**
 * Adds a class typed as `Name` or `Parent:Name` (the parent must exist); returns the new class.
 * A class of that name already on the list is returned instead of a duplicate.
 */
async function quickAddClass(classes, typed) {
  const fullName = typed.trim().replace(/\s*:\s*/g, ':');
  const existing = classes.find((c) => c.fullName.toLocaleLowerCase() === fullName.toLocaleLowerCase());
  if (existing) return existing;
  const cut = fullName.lastIndexOf(':');
  const name = cut < 0 ? fullName : fullName.slice(cut + 1);
  let parentId = null;
  if (cut >= 0) {
    const parentName = fullName.slice(0, cut).toLocaleLowerCase();
    const parent = classes.find((c) => c.fullName.toLocaleLowerCase() === parentName);
    if (!parent) throw new Error(`There is no class “${fullName.slice(0, cut)}” to put it under. Add that class first.`);
    parentId = parent.id;
  }
  return api.createClass({ name, parentId });
}

/**
 * Class dropdown: active classes indented under their parents. A line keeps a class chosen
 * earlier even if it has since been made inactive, so that one is listed too (marked inactive).
 * The last entry, "＋ Add new class…", turns the box into a name field: Enter (or Add) adds the
 * class to the list and picks it, Esc goes back. `Parent:Name` adds a sub-class.
 */
const ClassSelect = forwardRef(function ClassSelect({ classes, value, onChange, className = 'qb-input w-full', ...rest }, ref) {
  const options = classes.filter((c) => c.isActive || c.id === value);
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState('');
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const add = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    try {
      const created = await quickAddClass(classes, name);
      onChange(created.id);
      setAdding(false);
      setName('');
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : errorText(err));
    } finally {
      setBusy(false);
    }
  };

  if (adding) {
    return (
      <span className="relative flex items-center gap-0.5">
        <input
          ref={ref}
          autoFocus
          className={className}
          value={name}
          placeholder="New class name"
          aria-label="New class name"
          aria-invalid={Boolean(error)}
          title={error ?? 'Enter adds the class, Esc cancels. Parent:Name adds a sub-class.'}
          onChange={(e) => {
            setName(e.target.value);
            setError(null);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              add();
            } else if (e.key === 'Escape') {
              e.preventDefault();
              e.stopPropagation();
              setAdding(false);
              setError(null);
            }
          }}
        />
        <button type="button" className="qb-btn qb-btn-primary h-[20px] flex-none px-1.5 text-xs" onClick={add} disabled={!name.trim() || busy}>
          Add
        </button>
        {error && <span className="absolute left-0 top-full z-40 mt-0.5 whitespace-nowrap rounded-xs bg-white px-1 text-xs text-qb-negative shadow-panel">{error}</span>}
      </span>
    );
  }
  return (
    <select
      ref={ref}
      value={value ?? ''}
      onChange={(e) => {
        if (e.target.value === ADD_NEW) return setAdding(true);
        onChange(e.target.value === '' ? null : Number(e.target.value));
      }}
      className={className}
      {...rest}
    >
      <option value="">—</option>
      {options.map((c) => (
        <option key={c.id} value={c.id}>
          {'\u00a0\u00a0'.repeat(c.depth)}
          {c.name}
          {c.isActive ? '' : ' (inactive)'}
        </option>
      ))}
      <option value={ADD_NEW}>＋ Add new class…</option>
    </select>
  );
});

export default ClassSelect;
