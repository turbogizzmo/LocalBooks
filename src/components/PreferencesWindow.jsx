// Settings › Preferences, laid out as QuickBooks Desktop's Edit › Preferences window: a list of
// categories, each with My Preferences and Company Preferences tabs. Company preferences are keys
// in `src-tauri/src/preferences.rs`; the data-entry warnings among them are enforced by
// `src-tauri/src/guards.rs`. My Preferences are in `lib/myPreferences.js`.

import { createContext, useContext, useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import DesktopViewPreferences from './DesktopViewPreferences.jsx';
import GeneralPreferences, { AccountingMyPreferences, ReportsMyPreferences } from './GeneralPreferences.jsx';
import { CheckingCompany, CheckingMy, ItemsCompany, SalesTaxCompany, Tax1099Company, TimeExpensesCompany } from './TaxTimePreferences.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatDate, todayIso } from '../lib/dates.js';
import { focusNextField } from '../lib/myPreferences.js';
import DateBox from './DateBox.jsx';
import { JOB_STATUSES } from '../lib/salesPrefs.js';

/** One preference field that commits to the database as soon as the user leaves it or picks a value. */
export function PrefField({ label, prefKey, prefs, hint, children }) {
  const [value, setValue] = useState(prefs[prefKey] ?? '');
  const [state, setState] = useState({ status: 'idle' });

  useEffect(() => setValue(prefs[prefKey] ?? ''), [prefs, prefKey]);

  const commit = async (next = value) => {
    if (next === (prefs[prefKey] ?? '')) return;
    setState({ status: 'saving' });
    try {
      const stored = await api.setPreference(prefKey, next);
      setValue(stored);
      setState({ status: 'saved' });
    } catch (err) {
      setState({ status: 'error', message: errorText(err) });
    }
  };

  return (
    <div className="grid grid-cols-[170px_minmax(0,1fr)_140px] items-center gap-2 border-b border-qb-border px-2 py-0.5 last:border-b-0">
      <label htmlFor={`pref-${prefKey}`} className="text-sm font-semibold text-qb-ink-muted">
        {label}
      </label>
      <div>
        {children({ id: `pref-${prefKey}`, value, setValue, commit, invalid: state.status === 'error' })}
        {hint && <div className="text-2xs text-qb-ink-faint">{hint}</div>}
      </div>
      <div className="truncate text-xs" title={state.message}>
        {state.status === 'saving' && <span className="text-qb-ink-muted">Saving…</span>}
        {state.status === 'saved' && <span className="font-semibold text-qb-green-active">✓ Saved</span>}
        {state.status === 'error' && <span className="text-qb-negative">{state.message}</span>}
      </div>
    </div>
  );
}

export const textInput = (extra = {}) =>
  function TextInput({ id, value, setValue, commit, invalid }) {
    return (
      <input
        id={id}
        className="qb-input w-full"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => commit()}
        // Enter saves the field and moves on to the next one, as Tab does.
        onKeyDown={(e) => {
          if (e.key !== 'Enter') return;
          e.preventDefault();
          if (!focusNextField(e.currentTarget)) e.currentTarget.blur();
        }}
        aria-invalid={invalid}
        {...extra}
      />
    );
  };

export function Group({ title, id, children }) {
  return (
    <section className="qb-panel" id={id}>
      <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">{title}</h2>
      {children}
    </section>
  );
}

const SET_AT = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeStyle: 'short' });

/** Closing date: entries on or before it cannot be posted or voided; changes need the passcode. */
function ClosingDate() {
  const { data, error, reload } = useLedgerQuery(api.getClosingStatus);
  const [date, setDate] = useState('');
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [state, setState] = useState(null);

  useEffect(() => setDate(data?.lockDate ?? ''), [data?.lockDate]);

  const save = async (lockDate) => {
    setState({ busy: true });
    try {
      await api.setClosingDate({ lockDate: lockDate || null, currentPasscode: current || null, newPasscode: next === '' ? null : next });
      setCurrent('');
      setNext('');
      setState({ kind: 'success', text: lockDate ? `Books closed through ${formatDate(lockDate)}.` : 'All periods reopened.' });
      reload();
    } catch (err) {
      setState({ kind: 'error', text: errorText(err) });
    }
  };

  if (error) return <Notice kind="error" text={error} />;
  if (!data) return null;
  return (
    <Group title="Closing date" id="closing-date">
      <div className="flex flex-wrap items-end gap-3 px-2 py-1.5 text-sm">
        <div className="min-w-[200px]">
          <div className="text-2xs uppercase tracking-wide text-qb-ink-faint">In force</div>
          <div className="font-semibold">
            {data.lockDate ? `Closed through ${formatDate(data.lockDate)}` : 'No closing date: every period is open'}
            {data.passcodeProtected && <span className="ml-1 text-xs font-normal text-qb-ink-muted">· passcode protected</span>}
          </div>
        </div>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Closing date</span>
          <DateBox className="qb-input" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        {data.passcodeProtected && (
          <label className="flex flex-col gap-0.5 text-xs">
            <span className="font-semibold text-qb-ink-muted">Current passcode</span>
            <input type="password" className="qb-input w-[150px]" value={current} onChange={(e) => setCurrent(e.target.value)} autoComplete="off" />
          </label>
        )}
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">{data.passcodeProtected ? 'New passcode (optional)' : 'Passcode (optional)'}</span>
          <input type="password" className="qb-input w-[150px]" value={next} onChange={(e) => setNext(e.target.value)} autoComplete="new-password" />
        </label>
        <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={state?.busy || !date} onClick={() => save(date)}>
          Close books
        </button>
        {data.lockDate && (
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" disabled={state?.busy} onClick={() => save('')}>
            Reopen all periods
          </button>
        )}
      </div>
      <p className="border-t border-qb-border px-2 py-1 text-xs text-qb-ink-muted">
        Entries dated on or before the closing date cannot be posted or voided, from any screen, except by an External Accountant
        user. Changing or removing the date needs the passcode. Every change is kept in the history below, and anything posted or
        voided in a closed period is listed, with who did it, in Reports › Closing Date Exceptions.
      </p>
      {state?.text && (
        <div className="px-2 pb-1">
          <Notice kind={state.kind} text={state.text} onDismiss={() => setState(null)} />
        </div>
      )}
      {data.history.length > 0 && (
        <ul className="max-h-[96px] overflow-auto border-t border-qb-border px-2 py-1 text-xs scrollbar-thin">
          {data.history.map((h) => (
            <li key={h.id} className="flex gap-2">
              <span className="w-[150px] text-qb-ink-muted">{SET_AT.format(new Date(h.setAt))}</span>
              <span>{h.lockDate ? `Closed through ${formatDate(h.lockDate)}` : 'Reopened all periods'}</span>
              {h.passcodeProtected && <span className="text-qb-ink-muted">· passcode</span>}
            </li>
          ))}
        </ul>
      )}
    </Group>
  );
}

function TaxCalendar() {
  const { data, error } = useLedgerQuery(api.getTaxCalendar);
  const today = todayIso();
  const next = (data ?? []).find((d) => d.dueDate >= today);
  return (
    <Group title="Tax calendar">
      {error && <Notice kind="error" text={error} />}
      <table className="qb-grid">
        <colgroup>
          <col className="w-[108px]" />
          <col className="w-[130px]" />
          <col />
          <col className="w-[84px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">Due</th>
            <th className="qb-grid-head">Form</th>
            <th className="qb-grid-head">Filing</th>
            <th className="qb-grid-head">Status</th>
          </tr>
        </thead>
        <tbody>
          {(data ?? []).map((d) => (
            <tr key={`${d.dueDate}-${d.label}`} className={`qb-grid-row ${d.isPast ? 'text-qb-ink-faint' : ''}`}>
              <td className="qb-grid-cell font-semibold" title={d.statutoryDate !== d.dueDate ? `Statutory date ${formatDate(d.statutoryDate)} falls on a weekend` : undefined}>
                {formatDate(d.dueDate)}
                {d.statutoryDate !== d.dueDate && <span className="ml-0.5 text-qb-ink-faint">*</span>}
              </td>
              <td className="qb-grid-cell">{d.form}</td>
              <td className="qb-grid-cell" title={d.label}>{d.label}</td>
              <td className="qb-grid-cell">
                {d.isPast ? 'Past' : next && d.dueDate === next.dueDate && d.label === next.label ? <span className="font-semibold text-qb-warning">● Next</span> : 'Upcoming'}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="px-2 py-0.5 text-2xs text-qb-ink-muted">
        Federal due dates for the configured entity type and fiscal year. * Weekend dates move to the next Monday; federal holidays,
        disaster relief and extensions are not applied, and state deadlines are not included. Confirm dates with the IRS or your tax advisor.
      </p>
    </Group>
  );
}

function CompanyLogoField() {
  const { data: logo, reload } = useLedgerQuery(api.getCompanyLogo);
  const [state, setState] = useState({ status: 'idle' });

  const upload = async () => {
    const path = await open({ multiple: false, directory: false, filters: [{ name: 'Logo image', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp'] }] });
    if (!path) return;
    setState({ status: 'saving' });
    try {
      await api.setCompanyLogo(path);
      await reload();
      setState({ status: 'saved' });
    } catch (err) {
      setState({ status: 'error', message: errorText(err) });
    }
  };
  const remove = async () => {
    try {
      await api.clearCompanyLogo();
      await reload();
      setState({ status: 'saved' });
    } catch (err) {
      setState({ status: 'error', message: errorText(err) });
    }
  };

  return (
    <div className="grid grid-cols-[170px_minmax(0,1fr)_140px] items-center gap-2 border-b border-qb-border px-2 py-1">
      <span className="text-sm font-semibold text-qb-ink-muted">Logo</span>
      <div className="flex items-center gap-2">
        <div className="grid h-[44px] w-[132px] place-items-center rounded-xs border border-dashed border-qb-border-strong bg-qb-canvas">
          {logo ? <img src={logo} alt="Company logo" className="max-h-[40px] max-w-[124px] object-contain" /> : <span className="text-2xs text-qb-ink-faint">No logo</span>}
        </div>
        <div className="flex flex-col gap-0.5">
          <span className="flex gap-1">
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={upload}>
              {logo ? 'Replace…' : 'Upload logo…'}
            </button>
            {logo && (
              <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={remove}>
                Remove
              </button>
            )}
          </span>
          <span className="text-2xs text-qb-ink-faint">PNG, JPEG, GIF or WebP under 512 KB · printed on invoices, estimates and reports</span>
        </div>
      </div>
      <div className="truncate text-xs" title={state.message}>
        {state.status === 'saving' && <span className="text-qb-ink-muted">Saving…</span>}
        {state.status === 'saved' && <span className="font-semibold text-qb-green-active">✓ Saved</span>}
        {state.status === 'error' && <span className="text-qb-negative">{state.message}</span>}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------------------------------------
// Company preference controls
// ------------------------------------------------------------------------------------------------

const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const CURRENCY_PRESETS = ['$', '€', '£', '¥', 'CA$', 'A$', '₹', 'CHF', 'R$'];

/** A true/false company preference as one checkbox. */
const flag = (text) =>
  function Flag({ id, value, commit }) {
    return (
      <label className="flex items-center gap-1.5 text-sm">
        <input id={id} type="checkbox" className="accent-qb-green" checked={value === 'true'} onChange={(e) => commit(e.target.checked ? 'true' : 'false')} />
        {text}
      </label>
    );
  };

/** One of a few values, as a drop-down: `options` is `[[value, label], …]`. */
const choice = (options, width = 'w-[260px]') =>
  function Choice({ id, value, commit, invalid }) {
    return (
      <select id={id} className={`qb-input ${width}`} value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
        {options.map(([v, label]) => (
          <option key={v} value={v}>{label}</option>
        ))}
      </select>
    );
  };

/** A number of days (0 turns the warning off). */
const days = textInput({ inputMode: 'numeric', className: 'qb-input num w-[70px]' });

/** Something LocalBooks always does, shown where people look for the option. */
function Always({ label, children }) {
  return (
    <div className="grid grid-cols-[170px_minmax(0,1fr)_140px] items-center gap-2 border-b border-qb-border px-2 py-0.5 last:border-b-0">
      <span className="text-sm font-semibold text-qb-ink-muted">{label}</span>
      <span className="text-sm">
        <span className="mr-1 font-semibold text-qb-green-active">✓ Always</span>
        <span className="text-2xs text-qb-ink-faint">{children}</span>
      </span>
      <span />
    </div>
  );
}

/** Links to the screens where a category's other settings live. */
function SeeAlso({ navigate, links }) {
  if (!links?.length) return null;
  return (
    <section className="qb-panel">
      <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Set on their own screens</h2>
      <ul className="px-2 py-1 text-sm">
        {links.map(({ label, view, action = null, data = null, hint }) => (
          <li key={label} className="py-0.5">
            <button type="button" className="text-qb-link hover:underline" onClick={() => navigate(view, action, data)}>
              {label} ›
            </button>
            {hint && <span className="ml-1.5 text-2xs text-qb-ink-faint">{hint}</span>}
          </li>
        ))}
      </ul>
    </section>
  );
}

function Nothing({ children }) {
  return <p className="qb-panel px-2 py-1.5 text-sm text-qb-ink-muted">{children}</p>;
}

function CompanyInfo({ field }) {
  return (
    <Group title="Company information">
      {field({ label: 'Company name', prefKey: 'company_name', hint: 'Shown on reports, in the sidebar and in the window title', children: textInput({ maxLength: 100 }) })}
      <CompanyLogoField />
      {field({ label: 'Legal name', prefKey: 'company_legal_name', children: textInput({ maxLength: 150 }) })}
      {field({ label: 'EIN', prefKey: 'company_ein', hint: 'Federal employer identification number', children: textInput({ placeholder: 'XX-XXXXXXX', maxLength: 10 }) })}
      {field({ label: 'Address', prefKey: 'company_address', children: textInput({ maxLength: 300 }) })}
      {field({ label: 'Email', prefKey: 'company_email', children: textInput({ type: 'email', maxLength: 150 }) })}
      {field({ label: 'Phone', prefKey: 'company_phone', children: textInput({ type: 'tel', maxLength: 40 }) })}
    </Group>
  );
}

function GeneralCompany({ field }) {
  return (
    <>
      <Group title="Display">
        {field({ label: 'Years', prefKey: 'four_digit_years', hint: 'Off shows dates as 10/13/26', children: flag('Always show years as 4 digits (10/13/2026)') })}
        {field({
          label: 'Time format',
          prefKey: 'hours_format',
          hint: 'How hours show in time tracking and timesheets; you can type either way',
          children: ({ id, value, commit, invalid }) => (
            <select id={id} className="qb-input w-[220px]" value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
              <option value="hhmm">Minutes (1:30)</option>
              <option value="decimal">Decimal (1.50)</option>
            </select>
          ),
        })}
      </Group>
      <CompanyInfo field={field} />
      <Group title="Automatic backups">
        {field({ label: 'Daily backup', prefKey: 'auto_backup_enabled', hint: 'Taken when LocalBooks starts, at most once a day', children: flag('Back up the company file automatically') })}
        {field({ label: 'Keep', prefKey: 'auto_backup_keep', hint: 'Number of daily backups to keep (1–60)', children: textInput({ inputMode: 'numeric', className: 'qb-input num w-[70px]' }) })}
      </Group>
    </>
  );
}

function AccountingCompany({ field, forms }) {
  return (
    <>
      <Group title="Accounts">
        {field({
          label: 'First month of fiscal year',
          prefKey: 'fiscal_year_start',
          hint: 'Drives fiscal report periods and when Net Income closes into Retained Earnings',
          children: ({ id, value, commit, invalid }) => (
            <select id={id} className="qb-input w-[180px]" value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
              {MONTHS.map((m, i) => (
                <option key={m} value={`${String(i + 1).padStart(2, '0')}-01`}>{m}</option>
              ))}
            </select>
          ),
        })}
        {field({
          label: 'Tax form',
          prefKey: 'tax_form',
          hint: 'Sets the federal filing deadlines in the tax calendar',
          children: ({ id, value, commit, invalid }) => (
            <select id={id} className="qb-input w-full" value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
              {(forms ?? []).map(([code, label]) => (
                <option key={code} value={code}>{label}</option>
              ))}
            </select>
          ),
        })}
        {field({ label: 'Account numbers', prefKey: 'show_account_numbers', hint: 'Number accounts in Chart of Accounts; reports show the number before the name', children: flag('Show account numbers in reports') })}
        {field({ label: 'Subaccounts', prefKey: 'lowest_subaccount_only', hint: 'Electric instead of Utilities:Electric in the trial balance and general ledger', children: flag('Show lowest subaccount only') })}
        <Always label="Account on every line">Every line of every transaction needs an account before it can be saved; nothing posts to an uncategorized account.</Always>
        <Always label="Journal entry numbers">Each entry gets the next number when it is posted; numbers are never reused.</Always>
      </Group>
      <Group title="Warnings" id="date-warnings">
        {field({ label: 'Retained Earnings', prefKey: 'warn_retained_earnings', hint: 'LocalBooks moves each year’s profit there itself', children: flag('Warn when posting a transaction to Retained Earnings') })}
        {field({ label: 'Dates in the past', prefKey: 'warn_date_past_days', hint: 'Warn when a date is more than this many days before today (0 = never)', children: days })}
        {field({ label: 'Dates in the future', prefKey: 'warn_date_future_days', hint: 'Warn when a date is more than this many days after today (0 = never)', children: days })}
        <p className="px-2 py-0.5 text-2xs text-qb-ink-faint">A warning asks before saving, and Save anyway records the transaction as entered.</p>
      </Group>
      <Group title="Class tracking" id="class-tracking">
        {field({ label: 'Use class tracking', prefKey: 'class_tracking', hint: 'Adds a Class column to sales, purchase, deposit and journal entry forms (manage the list in Accounting › Classes)', children: flag('Track classes on transactions') })}
        <ClassOptions field={field} />
      </Group>
      <ClosingDate />
      <TaxCalendar />
    </>
  );
}

function ClassOptions({ field }) {
  const { prefs } = usePrefs();
  if (prefs.class_tracking !== 'true') return null;
  return (
    <>
      {field({
        label: 'Assign classes',
        prefKey: 'class_assign',
        hint: 'One class per line, or one class for the whole transaction',
        children: ({ id, value, commit, invalid }) => (
          <select id={id} className="qb-input w-[220px]" value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
            <option value="line">One to each line</option>
            <option value="transaction">One to the entire transaction</option>
          </select>
        ),
      })}
      {field({ label: 'Warn when no class', prefKey: 'class_prompt', hint: 'Asks before saving income or expense lines that have no class', children: flag('Warn me when a transaction is not assigned a class') })}
    </>
  );
}

function FinanceChargeCompany({ field, accounts }) {
  return (
    <Group title="Finance charges" id="finance-charges">
      {field({
        label: 'Annual interest rate',
        prefKey: 'finance_charge_annual_rate',
        hint: 'Percent a year, charged per day on overdue balances (a year counts as 365 days)',
        children: ({ id, value, setValue, commit, invalid }) => (
          <span className="flex items-center gap-1">
            <input id={id} className="qb-input num w-[80px]" value={value} inputMode="decimal" onChange={(e) => setValue(e.target.value)} onBlur={() => commit()} onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()} aria-invalid={invalid} />
            <span className="text-sm">%</span>
          </span>
        ),
      })}
      {field({ label: 'Minimum charge', prefKey: 'finance_charge_minimum', hint: 'A smaller charge is raised to this amount', children: textInput({ inputMode: 'decimal', className: 'qb-input num w-[100px]' }) })}
      {field({ label: 'Grace period', prefKey: 'finance_charge_grace_days', hint: 'Days past the due date before an invoice is charged; interest still runs from the due date', children: textInput({ inputMode: 'numeric', className: 'qb-input num w-[70px]' }) })}
      {field({
        label: 'Finance charge account',
        prefKey: 'finance_charge_account_id',
        hint: 'The income account finance charge invoices credit',
        children: ({ id, value, commit, invalid }) => (
          <AccountSelect
            id={id}
            accounts={accounts ?? []}
            filter={(a) => a.classification === 'INCOME'}
            value={value === '' ? null : Number(value)}
            onChange={(next) => commit(next === null ? '' : String(next))}
            placeholder="Choose an income account"
            aria-invalid={invalid}
          />
        ),
      })}
      {field({
        label: 'Calculate charges from',
        prefKey: 'finance_charge_from',
        hint: 'Interest runs from this date; an invoice is overdue by its due date and the grace period either way',
        children: ({ id, value, commit, invalid }) => (
          <select id={id} className="qb-input w-[180px]" value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
            <option value="due_date">Due date</option>
            <option value="invoice_date">Invoice date</option>
          </select>
        ),
      })}
      {field({ label: 'Charge on finance charges', prefKey: 'finance_charge_on_finance_charges', hint: 'Whether overdue finance charge invoices themselves earn finance charges', children: flag('Assess finance charges on overdue finance charges') })}
    </Group>
  );
}

const CASH_FLOW_SECTION = { OPERATING: 'Operating', INVESTING: 'Investing', FINANCING: 'Financing' };
/** The section each type goes in unless moved (mirrors `statements::default_cash_flow_section`). */
const DEFAULT_SECTION = {
  ACCOUNTS_RECEIVABLE: 'OPERATING',
  OTHER_CURRENT_ASSET: 'OPERATING',
  ACCOUNTS_PAYABLE: 'OPERATING',
  CREDIT_CARD: 'OPERATING',
  OTHER_CURRENT_LIABILITY: 'OPERATING',
  FIXED_ASSET: 'INVESTING',
  OTHER_ASSET: 'INVESTING',
  LONG_TERM_LIABILITY: 'FINANCING',
  EQUITY: 'FINANCING',
};

/** Which accounts go in each Statement of Cash Flows section. */
function CashFlowMapping({ accounts }) {
  const { prefs } = usePrefs();
  const [state, setState] = useState(null);
  const parse = (text) => {
    try {
      return text ? JSON.parse(text) : {};
    } catch {
      return {};
    }
  };
  // Follows the stored preference (another window's change shows here too); a change shows at
  // once and is put back if saving fails.
  const [map, setMap] = useState(() => parse(prefs.cash_flow_sections));
  useEffect(() => setMap(parse(prefs.cash_flow_sections)), [prefs.cash_flow_sections]);
  const rows = (accounts ?? []).filter((a) => DEFAULT_SECTION[a.accountType] && (a.isActive || map[a.id]));
  const save = async (next) => {
    const before = map;
    setMap(next);
    setState({ status: 'saving' });
    try {
      await api.setPreference('cash_flow_sections', Object.keys(next).length ? JSON.stringify(next) : '');
      setState({ status: 'saved' });
    } catch (err) {
      setMap(before);
      setState({ status: 'error', message: errorText(err) });
    }
  };
  const choose = (account, section) => {
    const next = { ...map };
    if (section === DEFAULT_SECTION[account.accountType]) delete next[account.id];
    else next[account.id] = section;
    save(next);
  };
  return (
    <Group title="Statement of Cash Flows" id="cash-flow-mapping">
      <p className="px-2 py-1 text-xs text-qb-ink-muted">
        Which section each balance sheet account’s change goes in. Bank accounts are the cash itself, and income and expense accounts are in net income.
        {state?.status === 'saving' && <span className="ml-1">Saving…</span>}
        {state?.status === 'saved' && <span className="ml-1 font-semibold text-qb-green-active">✓ Saved</span>}
        {state?.status === 'error' && <span className="ml-1 text-qb-negative">{state.message}</span>}
      </p>
      <div className="max-h-[260px] overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Account</th>
              <th className="qb-grid-head w-[150px]">Type</th>
              <th className="qb-grid-head w-[150px]">Section</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((a) => {
              const section = map[a.id] ?? DEFAULT_SECTION[a.accountType];
              return (
                <tr key={a.id} className="qb-grid-row">
                  <td className="qb-grid-cell truncate">{a.accountNumber ? `${a.accountNumber} · ${a.fullName}` : a.fullName}</td>
                  <td className="qb-grid-cell text-qb-ink-muted">{a.accountTypeName}</td>
                  <td className="qb-grid-cell">
                    <select aria-label={`Cash flow section for ${a.fullName}`} className={`qb-input w-full ${map[a.id] ? 'font-semibold' : ''}`} value={section} onChange={(e) => choose(a, e.target.value)}>
                      {Object.entries(CASH_FLOW_SECTION).map(([v, label]) => (
                        <option key={v} value={v}>{label}{v === DEFAULT_SECTION[a.accountType] ? ' (default)' : ''}</option>
                      ))}
                    </select>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {Object.keys(map).length > 0 && (
        <div className="border-t border-qb-border px-2 py-1">
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => save({})}>
            Revert to defaults
          </button>
        </div>
      )}
    </Group>
  );
}

function ReportsCompany({ field, accounts }) {
  return (
    <>
      <Group title="Report defaults" id="report-defaults">
        {field({ label: 'Summary reports basis', prefKey: 'report_basis', hint: 'Profit and Loss and the Income & Expense graph open on this basis; each report can still switch', children: choice([['accrual', 'Accrual'], ['cash', 'Cash']], 'w-[140px]') })}
        {field({ label: 'Aging reports', prefKey: 'aging_basis', hint: 'A/R and A/P Aging count days from this date', children: choice([['due', 'Age from due date'], ['transaction', 'Age from transaction date']]) })}
        {field({ label: 'Items', prefKey: 'report_items_show', hint: 'Sales by Item, Inventory Valuation and Stock Status', children: choice([['name', 'Name only'], ['description', 'Description only'], ['both', 'Name and description']]) })}
        <p className="px-2 py-0.5 text-2xs text-qb-ink-faint">Account numbers and subaccounts are under Accounting › Company Preferences. The Balance Sheet is always on the accrual basis.</p>
      </Group>
      <CashFlowMapping accounts={accounts} />
    </>
  );
}

function CurrenciesCompany({ field }) {
  return (
    <Group title="Currencies">
      {field({
        label: 'Home currency',
        prefKey: 'home_currency_code',
        hint: 'Every report and balance is in this currency; it locks once foreign-currency transactions exist',
        children: ({ id, value, setValue, commit, invalid }) => (
          <input id={id} className="qb-input w-[70px] uppercase" value={value} maxLength={3} onChange={(e) => setValue(e.target.value.toUpperCase())} onBlur={() => commit()} aria-invalid={invalid} />
        ),
      })}
      {field({
        label: 'Currency symbol',
        prefKey: 'currency_symbol',
        hint: 'Display only; amounts are stored in cents and never converted',
        children: ({ id, value, setValue, commit, invalid }) => (
          <span className="flex gap-1">
            <input id={id} className="qb-input w-[70px]" value={value} maxLength={4} onChange={(e) => setValue(e.target.value)} onBlur={() => commit()} aria-invalid={invalid} />
            {CURRENCY_PRESETS.map((c) => (
              <button key={c} type="button" className={`qb-btn qb-btn-secondary h-input px-1.5 ${value === c ? 'border-qb-green text-qb-green-active' : ''}`} onClick={() => commit(c)}>
                {c}
              </button>
            ))}
          </span>
        ),
      })}
      {field({
        label: 'Exchange rates',
        prefKey: 'exchange_rate_lookup',
        hint: (
          <>
            Suggests the European Central Bank’s rate for a foreign-currency transaction’s date, through the Frankfurter service
            (api.frankfurter.dev). Only the two currency codes and the date are sent; nothing about your company or its transactions.
            Rates are saved in the company file, so a date is looked up once and works offline. You can always type a different rate,
            and saved transactions keep theirs.
          </>
        ),
        children: flag('Look up daily exchange rates'),
      })}
    </Group>
  );
}

// ------------------------------------------------------------------------------------------------
// Batch 4: Sales & Customers, Jobs & Estimates, Payments, Bills, Send Forms
// ------------------------------------------------------------------------------------------------

function ShipMethodChoice({ id, value, commit, invalid }) {
  const methods = useLedgerQuery(api.listShipMethods).data ?? [];
  return (
    <select id={id} className="qb-input w-[220px]" value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
      <option value="">None</option>
      {methods
        .filter((m) => m.isActive || String(m.id) === value)
        .map((m) => (
          <option key={m.id} value={String(m.id)}>{m.name}</option>
        ))}
    </select>
  );
}

function SalesCompany({ field }) {
  return (
    <>
      <Group title="Sales forms">
        {field({ label: 'Usual shipping method', prefKey: 'default_ship_method_id', hint: 'Filled in as Ship Via on new invoices, sales receipts and estimates (add methods in Reps, Ship Via & Fields)', children: ShipMethodChoice })}
        {/* Usual F.O.B. (`default_fob`) goes here once the F.O.B. box (#158) is on sales forms. */}
        {field({
          label: 'Default markup',
          prefKey: 'default_markup_percent',
          hint: 'The markup Change Item Prices starts with when new prices are worked out from cost',
          children: ({ id, value, setValue, commit, invalid }) => (
            <span className="flex items-center gap-1">
              <input id={id} className="qb-input num w-[80px]" value={value} inputMode="decimal" onChange={(e) => setValue(e.target.value)} onBlur={() => commit()} onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()} aria-invalid={invalid} />
              <span className="text-sm">%</span>
            </span>
          ),
        })}
        <Always label="Invoice numbers">Each invoice gets the next number, so no two share one.</Always>
      </Group>
      <Group title="Pricing">
        {field({ label: 'Price levels', prefKey: 'price_levels_enabled', hint: 'Off hides price levels on customers and forms, and every form uses standard prices; levels you made are kept', children: flag('Use price levels') })}
      </Group>
      <Group title="Warnings">
        {field({ label: 'Overdue customers', prefKey: 'warn_customer_overdue', hint: 'Asks before saving a new invoice or sales receipt for a customer (or job) with invoices past due', children: flag('Warn before selling to a customer with overdue invoices') })}
        {field({ label: 'Open invoices', prefKey: 'warn_customer_open_invoices', hint: 'Asks before saving a new sales receipt for a customer (or job) with invoices still open: a receipt doesn’t pay them, Receive Payments does', children: flag('Warn before a sales receipt from a customer with open invoices') })}
      </Group>
      {/* Estimates to invoices › Zero-amount lines (`estimate_drop_zero_lines`) shows once estimates take 0.00 lines (#151). */}
      <Nothing>Sales orders and the Collections Center are not part of LocalBooks, so their options are not here.</Nothing>
    </>
  );
}

function JobsCompany({ field }) {
  return (
    <>
      <Group title="Estimates">
        {field({ label: 'Estimates', prefKey: 'estimates_enabled', hint: 'Off hides Estimates from the menus, the side bar and New; estimates already saved are kept', children: flag('Create estimates') })}
        <Always label="Estimate numbers">Each estimate gets the next number, so no two share one.</Always>
      </Group>
      <Group title="Job status names">
        {JOB_STATUSES.map((st) =>
          field({ key: st.key, label: st.label, prefKey: st.key, hint: st.code === 'PENDING' ? 'How each status reads on jobs and in the customer list' : undefined, children: textInput({ maxLength: 30, className: 'qb-input w-[220px]' }) }),
        )}
      </Group>
      <Nothing>
        Progress invoicing (invoicing an estimate in parts) is not available yet: an estimate becomes one invoice, which you can change in Create
        Invoices before saving it.
      </Nothing>
    </>
  );
}

function PaymentsCompany({ field, accounts }) {
  return (
    <>
      <Group title="Receive Payments">
        {field({
          label: 'Apply payments',
          prefKey: 'payment_auto_apply',
          hint: 'How an amount received is spread over the customer’s open invoices; Auto Apply on the form always applies',
          children: choice([
            ['exact', 'Automatically: an exact match, else oldest first'],
            ['oldest', 'Automatically: oldest invoices first'],
            ['off', 'Not automatically: I tick the invoices'],
          ]),
        })}
        {field({ label: 'Calculate the amount', prefKey: 'payment_auto_calculate', hint: 'The amount received becomes the total of the invoices ticked or paid', children: flag('Work out the payment amount from the invoices ticked') })}
        {field({ label: 'Deposit to', prefKey: 'payment_undeposited_default', hint: 'Off: payments and sales receipts go to the first bank account unless you choose another', children: flag('Use Undeposited Funds as the usual deposit account') })}
      </Group>
      <Group title="Early-payment discounts">
        {field({
          label: 'Discounts',
          prefKey: 'payment_auto_apply_discounts',
          hint: 'Receive Payments takes the discount an invoice’s terms offer (such as 2% 10 Net 30) when it is paid in full by the last day; off, you type it on the invoice’s row',
          children: flag('Take customers’ early-payment discounts automatically'),
        })}
        {field({
          label: 'Discount account',
          prefKey: 'payment_discount_account_id',
          hint: 'Discounts customers take are debited here, usually an income account such as Sales Discounts; without one, Receive Payments takes no discounts',
          children: ({ id, value, commit, invalid }) => (
            <AccountSelect
              id={id}
              accounts={accounts ?? []}
              filter={(a) => a.isActive && !a.isSystem && (a.classification === 'INCOME' || a.classification === 'EXPENSE')}
              value={value === '' ? null : Number(value)}
              onChange={(next) => commit(next === null ? '' : String(next))}
              placeholder="Choose an income or expense account"
              aria-invalid={invalid}
            />
          ),
        })}
      </Group>
    </>
  );
}

function BillsCompany({ field, accounts }) {
  return (
    <Group title="Bills">
      {field({ label: 'Bills are due', prefKey: 'bill_due_days', hint: 'Days after the bill date, for a vendor without terms (0 = on receipt); a vendor’s own terms come first', children: days })}
      {field({ label: 'Duplicate bill numbers', prefKey: 'warn_duplicate_bill_numbers', hint: 'Per vendor: two vendors may use the same number', children: flag('Warn about duplicate bill numbers from the same vendor') })}
      {field({ label: 'Open bills', prefKey: 'warn_vendor_open_bills', hint: 'Asks before saving a new check or expense to a vendor with unpaid bills: it doesn’t pay them, Pay Bills does', children: flag('Warn before paying a vendor directly while it has open bills') })}
      {field({ label: 'Vendor credits', prefKey: 'bill_auto_apply_credits', hint: 'Pay Bills uses the vendor’s open credits on the bills ticked, oldest credit first, and pays the rest', children: flag('Use vendor credits automatically') })}
      {field({ label: 'Discounts', prefKey: 'bill_auto_apply_discounts', hint: 'Pay Bills takes the early-payment discount a bill’s terms offer (such as 2% 10 Net 30) when paid by its last day', children: flag('Take early-payment discounts automatically') })}
      {field({
        label: 'Discount account',
        prefKey: 'bill_discount_account_id',
        hint: 'Discounts taken are credited here, such as Purchase Discounts or Cost of Goods Sold',
        children: ({ id, value, commit, invalid }) => (
          <AccountSelect
            id={id}
            accounts={accounts ?? []}
            filter={(a) => a.isActive && !a.isSystem && a.classification === 'EXPENSE'}
            value={value === '' ? null : Number(value)}
            onChange={(next) => commit(next === null ? '' : String(next))}
            placeholder="Choose an expense account"
            aria-invalid={invalid}
          />
        ),
      })}
    </Group>
  );
}

function SendFormsCompany({ field }) {
  return (
    <Group title="Sending forms">
      {field({
        label: 'New customers',
        prefKey: 'default_delivery_method',
        hint: 'A new customer’s preferred delivery, which ticks Email later or Print later on their new forms; change it on each customer',
        children: choice([
          ['none', 'None'],
          ['email', 'Email (marked Email later)'],
          ['print', 'Print (marked Print later)'],
        ]),
      })}
    </Group>
  );
}

// ------------------------------------------------------------------------------------------------
// The categories
// ------------------------------------------------------------------------------------------------

/**
 * Each category as in QuickBooks Desktop's Edit › Preferences: `my` and `company` render its two
 * tabs (null when the tab has no options), `links` go to the screens that hold its other settings.
 */
export const CATEGORIES = [
  {
    id: 'accounting',
    label: 'Accounting',
    my: () => <AccountingMyPreferences />,
    company: (p) => <AccountingCompany {...p} />,
    links: [
      { label: 'Classes', view: 'accounting/classes' },
      { label: 'Chart of Accounts', view: 'accounting/chart-of-accounts', hint: 'account numbers and subaccounts' },
    ],
  },
  {
    id: 'bills',
    label: 'Bills',
    company: (p) => <BillsCompany {...p} />,
    links: [
      { label: 'Bills', view: 'expenses/bills' },
      { label: 'Vendors', view: 'expenses/vendors', hint: 'each vendor’s terms' },
    ],
  },
  {
    id: 'checking',
    label: 'Checking',
    my: () => <CheckingMy />,
    company: (p) => <CheckingCompany {...p} />,
    links: [
      { label: 'Expenses & Checks', view: 'expenses/expenses', hint: 'the next check number is filled in for each account' },
      { label: 'Bank Feeds', view: 'banking/feeds', hint: 'renaming rules for downloaded payees' },
    ],
  },
  {
    id: 'desktop',
    label: 'Desktop View',
    my: () => <DesktopViewPreferences />,
    company: null,
  },
  {
    id: 'finance-charge',
    label: 'Finance Charge',
    company: (p) => <FinanceChargeCompany {...p} />,
    links: [{ label: 'Assess Finance Charges', view: 'sales/finance-charges' }],
  },
  {
    id: 'general',
    label: 'General',
    my: () => <GeneralPreferences />,
    company: (p) => <GeneralCompany {...p} />,
    links: [
      { label: 'Users & Passwords', view: 'settings/users', hint: 'sign out after a period of no use' },
      { label: 'File & Data Controls', view: 'settings/data' },
    ],
  },
  {
    id: 'items',
    label: 'Items & Inventory',
    company: (p) => (
      <>
        <ItemsCompany {...p} />
        <Group title="Purchase orders">
          <Always label="Purchase order numbers">Each purchase order gets the next number, so no two share one.</Always>
        </Group>
        <Nothing>Units of measure (selling by the each, case or pallet) are not in LocalBooks: items have one unit.</Nothing>
      </>
    ),
    links: [
      { label: 'Products & Services', view: 'sales/products', hint: 'inventory items, reorder points and prices' },
      { label: 'Purchase Orders', view: 'expenses/purchase-orders' },
    ],
  },
  {
    id: 'jobs',
    label: 'Jobs & Estimates',
    company: (p) => <JobsCompany {...p} />,
    links: [
      { label: 'Estimates', view: 'sales/estimates' },
      { label: 'Customers', view: 'sales/customers', hint: 'each job’s status' },
    ],
  },
  {
    id: 'currencies',
    label: 'Multiple Currencies',
    company: (p) => <CurrenciesCompany {...p} />,
    links: [{ label: 'Currency Revaluation', view: 'accounting/fx-revaluation' }],
  },
  {
    id: 'payments',
    label: 'Payments',
    company: (p) => <PaymentsCompany {...p} />,
    links: [
      { label: 'Invoices', view: 'sales/invoices', hint: 'Receive Payments applies a payment to open invoices' },
      { label: 'Deposits & Transfers', view: 'banking/transfers', hint: 'payments wait in Undeposited Funds until deposited' },
    ],
  },
  {
    id: 'reminders',
    label: 'Reminders',
    links: [
      { label: 'Home page reminders', view: 'dashboard', hint: 'what to show, and how many days ahead' },
      { label: 'To Do List', view: 'accounting/todos' },
    ],
  },
  {
    id: 'reports',
    label: 'Reports & Graphs',
    my: () => <ReportsMyPreferences />,
    company: (p) => <ReportsCompany {...p} />,
    links: [{ label: 'Reports', view: 'reports', hint: 'each report keeps its own basis, dates and columns' }],
  },
  {
    id: 'sales',
    label: 'Sales & Customers',
    company: (p) => <SalesCompany {...p} />,
    links: [
      { label: 'Products & Services', view: 'sales/products', hint: 'price levels' },
      { label: 'Reps, Ship Via & Fields', view: 'sales/lists' },
      { label: 'Customers', view: 'sales/customers' },
    ],
  },
  {
    id: 'sales-tax',
    label: 'Sales Tax',
    company: (p) => <SalesTaxCompany {...p} />,
    links: [{ label: 'Sales Tax', view: 'sales/tax', hint: 'turn sales tax on, rates and the default rate' }],
  },
  {
    id: 'send-forms',
    label: 'Send Forms',
    company: (p) => <SendFormsCompany {...p} />,
    links: [
      { label: 'Email', view: 'settings/email', hint: 'how forms are sent' },
      { label: 'Custom Forms', view: 'settings/forms', hint: 'how forms look, and each form’s email subject and message' },
      { label: 'Send Forms', view: 'sales/send-forms', hint: 'forms waiting to be sent' },
    ],
  },
  {
    id: '1099',
    label: 'Tax: 1099',
    company: (p) => <Tax1099Company {...p} />,
    links: [
      { label: '1099 Summary', view: 'reports', data: { report: '1099' } },
      { label: '1099-NEC and 1096', view: 'reports', data: { report: '1099-nec' } },
      { label: 'Vendors', view: 'expenses/vendors', hint: 'tax IDs and which vendors get a 1099' },
    ],
  },
  {
    id: 'time',
    label: 'Time & Expenses',
    company: (p) => <TimeExpensesCompany {...p} />,
    links: [
      { label: 'Bill Costs', view: 'sales/bill-costs', hint: 'invoice billable expenses' },
      { label: 'Time Tracking', view: 'employees/time' },
      { label: 'Mileage', view: 'employees/mileage' },
    ],
  },
];

/** Help shortcuts and menu items (`intent.action`) to the category that holds them (Lists › Currency List: currencies). */
const ACTION_CATEGORY = { 'cash-flow': ['reports', 'company'], closing: ['accounting', 'company'], 'finance-charges': ['finance-charge', 'company'], 'class-tracking': ['accounting', 'company'], currencies: ['currencies', 'company'] };
const ANCHORS = { 'cash-flow': 'cash-flow-mapping', closing: 'closing-date', 'class-tracking': 'class-tracking' };
const REMEMBER = 'localbooks.preferencesCategory';

function remembered() {
  try {
    const id = window.localStorage.getItem(REMEMBER);
    return CATEGORIES.some((c) => c.id === id) ? id : 'general';
  } catch {
    return 'general';
  }
}

const PrefsContext = createContext(null);
const usePrefs = () => useContext(PrefsContext);

function CompanyTab({ category, navigate }) {
  const { prefs, error, forms, accounts } = usePrefs();
  if (error) return <Notice kind="error" text={error} />;
  if (!prefs) return <div className="px-2 py-1 text-sm text-qb-ink-muted">Loading…</div>;
  const field = ({ key, ...props }) => <PrefField key={key ?? props.prefKey} prefs={prefs} {...props} />;
  return (
    <>
      {category.company ? category.company({ field, forms, accounts }) : <Nothing>{category.label} has no company preferences here.</Nothing>}
      <SeeAlso navigate={navigate} links={category.links} />
    </>
  );
}

/**
 * Edit › Preferences: categories on the left, each with My Preferences (yours, on this computer)
 * and Company Preferences (everyone's, Admins only). Company options save as soon as you leave a
 * field; My Preferences take effect at once.
 */
export default function PreferencesWindow({ isAdmin, intent, navigate }) {
  const [categoryId, setCategoryId] = useState(() => ACTION_CATEGORY[intent?.action]?.[0] ?? remembered());
  const category = CATEGORIES.find((c) => c.id === categoryId) ?? CATEGORIES[0];
  const preferred = (c) => (c.my ? 'my' : isAdmin ? 'company' : 'my');
  const [tab, setTab] = useState(() => ACTION_CATEGORY[intent?.action]?.[1] ?? preferred(category));
  const { data: prefs, error } = useLedgerQuery(api.getPreferences);
  const { data: forms } = useLedgerQuery(api.listTaxForms);
  const { data: accounts } = useLedgerQuery(api.listAccounts);

  const choose = (c) => {
    setCategoryId(c.id);
    setTab(preferred(c));
    try {
      window.localStorage.setItem(REMEMBER, c.id);
    } catch {
      // Remembering the category is a convenience only.
    }
  };

  // Help shortcuts open their category and scroll to the section.
  useEffect(() => {
    const target = ACTION_CATEGORY[intent?.action];
    if (!target) return undefined;
    setCategoryId(target[0]);
    setTab(isAdmin ? target[1] : 'my');
    const anchor = ANCHORS[intent.action];
    if (!anchor) return undefined;
    const timer = setTimeout(() => document.getElementById(anchor)?.scrollIntoView({ behavior: 'smooth', block: 'start' }), 80);
    return () => clearTimeout(timer);
  }, [intent, isAdmin]);

  const tabButton = (id, label) => (
    <button
      type="button"
      role="tab"
      aria-selected={tab === id}
      onClick={() => setTab(id)}
      className={`-mb-px rounded-t-xs border px-3 py-0.5 text-sm font-semibold ${tab === id ? 'border-qb-border-strong border-b-white bg-white' : 'border-transparent text-qb-ink-muted hover:text-qb-ink'}`}
    >
      {label}
    </button>
  );

  return (
    <PrefsContext.Provider value={{ prefs, error, forms, accounts }}>
      <div className="grid min-h-0 flex-1 grid-cols-[170px_minmax(0,1fr)] gap-2">
        <nav aria-label="Preference categories" className="qb-panel min-h-0 overflow-auto py-0.5 scrollbar-thin">
          <ul role="listbox" aria-label="Categories">
            {CATEGORIES.map((c) => (
              <li key={c.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={c.id === category.id}
                  onClick={() => choose(c)}
                  className={`block w-full truncate px-2 py-0.5 text-left text-sm ${c.id === category.id ? 'bg-qb-nav font-semibold text-white' : 'hover:bg-qb-row-hover'}`}
                >
                  {c.label}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        <div className="flex min-h-0 flex-col">
          <div className="flex flex-none items-end gap-1 border-b border-qb-border-strong px-1" role="tablist" aria-label={`${category.label} preferences`}>
            {tabButton('my', 'My Preferences')}
            {tabButton('company', 'Company Preferences')}
            <h2 className="ml-auto self-center pb-0.5 text-md font-semibold">{category.label}</h2>
          </div>
          <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto pt-2 scrollbar-thin" role="tabpanel">
            {tab === 'my' && (category.my ? category.my() : <Nothing>{category.label} has no personal preferences{category.company ? '; see Company Preferences.' : '.'}</Nothing>)}
            {tab === 'my' && !category.company && <SeeAlso navigate={navigate} links={category.links} />}
            {tab === 'company' && (isAdmin ? <CompanyTab category={category} navigate={navigate} /> : <Notice kind="info" text="Company preferences are set by an Admin." />)}
          </div>
        </div>
      </div>
    </PrefsContext.Provider>
  );
}
