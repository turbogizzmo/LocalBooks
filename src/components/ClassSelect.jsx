import { forwardRef, useState } from 'react';
import { api, useLedgerQuery } from '../lib/api.js';

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

/** Whether a grid shows its Class column: tracking is on, per line, or a row already has a class. */
export function showsClassColumn(tracking, rows) {
  return (tracking.enabled && !tracking.perTransaction && tracking.hasClasses) || rows.some((r) => r.classId != null);
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
    tracking.perTransaction && tracking.hasClasses ? (
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

/**
 * Class dropdown: active classes indented under their parents. A line keeps a class chosen
 * earlier even if it has since been made inactive, so that one is listed too (marked inactive).
 */
const ClassSelect = forwardRef(function ClassSelect({ classes, value, onChange, className = 'qb-input w-full', ...rest }, ref) {
  const options = classes.filter((c) => c.isActive || c.id === value);
  return (
    <select
      ref={ref}
      value={value ?? ''}
      onChange={(e) => onChange(e.target.value === '' ? null : Number(e.target.value))}
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
    </select>
  );
});

export default ClassSelect;
