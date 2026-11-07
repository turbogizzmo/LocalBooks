// Preferences batch 5 (tax and time): the Company Preferences of Sales Tax, Time & Expenses,
// Tax: 1099, Items & Inventory and Checking, and Checking's My Preferences. The keys are in
// `src-tauri/src/preferences/tax_time.rs`, which says where each one changes behaviour.

import AccountSelect from './AccountSelect.jsx';
import { Group, textInput } from './PreferencesWindow.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { setMyPreferences, useMyPreferences } from '../lib/myPreferences.js';

const flag = (text) =>
  function Flag({ id, value, commit }) {
    return (
      <label className="flex items-center gap-1.5 text-sm">
        <input id={id} type="checkbox" className="accent-qb-green" checked={value === 'true'} onChange={(e) => commit(e.target.checked ? 'true' : 'false')} />
        {text}
      </label>
    );
  };

/** A choice among fixed values, as radio buttons on one line. */
const choice = (options) =>
  function Choice({ id, value, commit }) {
    return (
      <span className="flex flex-wrap gap-x-3" role="radiogroup" id={id}>
        {options.map(([v, label]) => (
          <label key={v} className="flex items-center gap-1 text-sm">
            <input type="radio" name={id} className="accent-qb-green" checked={value === v} onChange={() => commit(v)} />
            {label}
          </label>
        ))}
      </span>
    );
  };

/** An Income account (blank = none). */
const incomeAccount = (accounts, placeholder) =>
  function IncomeAccount({ id, value, commit, invalid }) {
    return (
      <AccountSelect
        id={id}
        accounts={accounts ?? []}
        filter={(a) => a.classification === 'INCOME'}
        value={value === '' ? null : Number(value)}
        onChange={(next) => commit(next === null ? '' : String(next))}
        placeholder={placeholder}
        aria-invalid={invalid}
      />
    );
  };

export function SalesTaxCompany({ field }) {
  return (
    <Group title="Sales tax" id="sales-tax-preferences">
      {field({
        label: 'Owe sales tax',
        prefKey: 'sales_tax_basis',
        hint: 'The Sales Tax report and what it says you owe follow this. On cash basis an invoice counts as it is paid.',
        children: choice([
          ['accrual', 'As of invoice date (accrual)'],
          ['cash', 'Upon receipt of payment (cash)'],
        ]),
      })}
      {field({
        label: 'File returns',
        prefKey: 'sales_tax_frequency',
        hint: 'Sets the Sales Tax report’s period and a reminder on the 20th after each period ends (check your state’s due date)',
        children: choice([
          ['monthly', 'Monthly'],
          ['quarterly', 'Quarterly'],
          ['yearly', 'Yearly'],
        ]),
      })}
      {field({ label: 'Taxable lines', prefKey: 'sales_tax_mark_taxable', hint: 'On printed and emailed invoices, sales receipts, credit memos and estimates', children: flag('Mark taxable amounts with a “T” when printing') })}
    </Group>
  );
}

const WEEKDAYS = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday'];

export function TimeExpensesCompany({ field, accounts }) {
  const percent = textInput({ inputMode: 'decimal', className: 'qb-input num w-[80px]' });
  return (
    <>
      <Group title="Time tracking" id="time-preferences">
        {field({
          label: 'First day of work week',
          prefKey: 'week_start_day',
          hint: 'Weekly timesheets start on this day',
          children: ({ id, value, commit, invalid }) => (
            <select id={id} className="qb-input w-[160px]" value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
              {WEEKDAYS.map((d) => (
                <option key={d} value={d}>
                  {d.charAt(0).toUpperCase() + d.slice(1)}
                </option>
              ))}
            </select>
          ),
        })}
        {field({ label: 'Billable time', prefKey: 'time_billable_default', hint: 'New time entries and timesheet rows start with Billable ticked', children: flag('Mark all time entries as billable') })}
        <p className="px-2 py-0.5 text-2xs text-qb-ink-faint">Whether hours show as 1:30 or 1.50 is under General › Company Preferences.</p>
      </Group>
      <Group title="Billable expenses" id="expense-preferences">
        {field({ label: 'Billable expenses', prefKey: 'expenses_billable_default', hint: 'Expense, check and bill lines given a customer start with Bill? ticked', children: flag('Mark all expenses as billable') })}
        {field({
          label: 'Reimbursed expenses',
          prefKey: 'reimbursed_as_income',
          hint: 'Off: billing a cost to the customer credits the expense account back. On: it is income, in the account below.',
          children: flag('Track reimbursed expenses as income'),
        })}
        {field({ label: 'Reimbursement income', prefKey: 'reimbursed_income_account_id', hint: 'Used when reimbursed expenses count as income', children: incomeAccount(accounts, 'Choose an income account') })}
        {field({ label: 'Default markup', prefKey: 'billable_markup_percent', hint: 'Percent added to billed costs; Bill Costs can change it each time', children: percent })}
        {field({ label: 'Markup account', prefKey: 'billable_markup_account_id', hint: 'The income account the markup is credited to', children: incomeAccount(accounts, 'Choose an income account') })}
      </Group>
    </>
  );
}

/** Expense accounts mapped to 1099 boxes, saved as one preference. */
function AccountMapping({ id, value, commit, accounts }) {
  const boxes = useLedgerQuery(api.list1099Boxes).data ?? [];
  let map = {};
  try {
    map = JSON.parse(value || '{}');
  } catch {
    map = {};
  }
  const expenses = (accounts ?? []).filter((a) => a.classification === 'EXPENSE' && (a.isActive || map[a.id]));
  const set = (accountId, code) => {
    const next = { ...map };
    if (code) next[accountId] = code;
    else delete next[accountId];
    commit(JSON.stringify(next));
  };
  return (
    <div id={id} className="max-h-[260px] overflow-auto rounded-xs border border-qb-border scrollbar-thin">
      <table className="qb-grid">
        <thead>
          <tr>
            <th className="qb-grid-head">Expense account</th>
            <th className="qb-grid-head">1099 box</th>
          </tr>
        </thead>
        <tbody>
          {expenses.map((a) => (
            <tr key={a.id} className="qb-grid-row">
              <td className="qb-grid-cell">{a.fullName ?? a.name}</td>
              <td className="qb-grid-cell-edit">
                <select value={map[a.id] ?? ''} onChange={(e) => set(a.id, e.target.value)} aria-label={`1099 box for ${a.fullName ?? a.name}`}>
                  <option value="">Not reported</option>
                  {boxes.map((b) => (
                    <option key={b.code} value={b.code}>
                      {b.form} {b.label}
                    </option>
                  ))}
                </select>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function Tax1099Company({ field, accounts }) {
  return (
    <Group title="1099 forms" id="tax-1099-preferences">
      {field({ label: 'Do you file 1099s?', prefKey: 'tax_1099_enabled', hint: 'Off hides the 1099 checkbox on vendors, the 1099 reports and the 1099 deadline', children: flag('Yes, track payments for 1099 forms') })}
      {field({
        label: 'Accounts to boxes',
        prefKey: 'tax_1099_accounts',
        hint: 'The 1099 Summary and Detail count payments to 1099 vendors by the accounts they paid. With nothing mapped, everything counts as nonemployee compensation (1099-NEC box 1).',
        children: (p) => <AccountMapping {...p} accounts={accounts} />,
      })}
      <p className="px-2 py-0.5 text-2xs text-qb-ink-faint">
        Thresholds go by the payment year: $600 before 2026 and $2,000 from 2026 for nonemployee compensation, rents, other income and medical payments; $10 for royalties;
        $600 for attorneys’ gross proceeds.
      </p>
    </Group>
  );
}

export function ItemsCompany({ field }) {
  return (
    <Group title="Inventory and purchase orders" id="inventory-preferences">
      {field({ label: 'Inventory', prefKey: 'inventory_enabled', hint: 'Off hides Purchase Orders and tracking stock on products; items already tracked keep their stock', children: flag('Inventory and purchase orders are active') })}
      {field({
        label: 'Not enough stock',
        prefKey: 'negative_stock',
        hint: 'When an invoice or sales receipt would take a tracked item below zero',
        children: choice([
          ['warn', 'Warn'],
          ['stop', 'Don’t allow the sale'],
          ['off', 'Say nothing'],
        ]),
      })}
    </Group>
  );
}

export function CheckingCompany({ field }) {
  return (
    <Group title="Checks">
      {field({ label: 'Duplicate check numbers', prefKey: 'warn_duplicate_check_numbers', hint: 'Per bank account; voided checks don’t count', children: flag('Warn about duplicate check numbers') })}
      {field({ label: 'Vendor account number', prefKey: 'check_memo_account_number', hint: 'A check left without a memo gets “Acct #…” from the vendor’s record', children: flag('Print the vendor’s account number in the check memo') })}
      {field({
        label: 'Check date',
        prefKey: 'date_checks_when_printed',
        hint: 'A check waiting in Print Forms is re-dated to the day it prints (an edit: the original is voided and kept in the audit log)',
        children: flag('Change the check date when a check is printed'),
      })}
    </Group>
  );
}

/** Checking › My Preferences: the account each form opens with, on this computer. */
export function CheckingMy() {
  const prefs = useMyPreferences();
  const accounts = useLedgerQuery(api.listAccounts).data ?? [];
  const row = (key, label, filter) => (
    <div className="grid grid-cols-[170px_minmax(0,1fr)] items-center gap-2 border-b border-qb-border px-2 py-0.5 last:border-b-0">
      <span className="text-sm font-semibold text-qb-ink-muted">{label}</span>
      <AccountSelect
        accounts={accounts}
        filter={(a) => a.isActive && filter(a)}
        value={accounts.some((a) => a.id === prefs[key]) ? prefs[key] : null}
        onChange={(next) => setMyPreferences({ [key]: next ?? 0 })}
        placeholder="The first bank account"
        aria-label={label}
      />
    </div>
  );
  return (
    <Group title="Open these forms with this account">
      {row('checkAccountId', 'Write Checks', (a) => a.accountType === 'BANK')}
      {row('payBillsAccountId', 'Pay Bills', (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD')}
      {row('depositAccountId', 'Make Deposits', (a) => a.accountType === 'BANK')}
      <p className="px-2 py-0.5 text-2xs text-qb-ink-faint">For you on this computer. You can still pick another account on each form.</p>
    </Group>
  );
}
