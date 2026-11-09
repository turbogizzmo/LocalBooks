import { forwardRef } from 'react';
import { formatCents } from '../lib/money.js';

/** Account dropdown grouped by account type, in chart-of-accounts order. */
const AccountSelect = forwardRef(function AccountSelect(
  { accounts, value, onChange, filter = () => true, placeholder, className = 'qb-input w-full', ...rest },
  ref,
) {
  const groups = [];
  for (const account of accounts) {
    if (!account.isActive || !filter(account)) continue;
    const last = groups[groups.length - 1];
    if (last && last.name === account.accountTypeName) last.accounts.push(account);
    else groups.push({ name: account.accountTypeName, accounts: [account] });
  }

  return (
    <select
      ref={ref}
      value={value ?? ''}
      onChange={(e) => onChange(e.target.value === '' ? null : Number(e.target.value))}
      className={className}
      {...rest}
    >
      {placeholder !== undefined && <option value="">{placeholder}</option>}
      {groups.map((group) => (
        <optgroup key={group.name} label={group.name}>
          {group.accounts.map((a) => (
            <option key={a.id} value={a.id}>
              {a.accountNumber ? `${a.accountNumber} · ${a.fullName}` : a.fullName}
            </option>
          ))}
        </optgroup>
      ))}
    </select>
  );
});

export default AccountSelect;

export function accountLabel(accounts, id) {
  const a = accounts.find((x) => x.id === id);
  if (!a) return '';
  return a.accountNumber ? `${a.accountNumber} · ${a.fullName}` : a.fullName;
}

/**
 * "Ending balance" beside a form's account box (Write Checks, card charges, Make Deposits, Pay
 * Bills), as the desktop forms show it: the account's balance as of now, on its normal side (what
 * the bank holds, what the card owes), including everything posted. Nothing until an account is
 * chosen. The account list refreshes after each save, so it follows what was just posted.
 */
export function EndingBalance({ accounts, accountId }) {
  const account = accountId === null || accountId === undefined ? null : accounts.find((a) => a.id === accountId);
  if (!account || typeof account.balanceCents !== 'number') return null;
  return (
    <span className="self-end pb-1 text-sm text-qb-ink-muted" aria-live="polite">
      Ending balance <span className={`num font-semibold ${account.balanceCents < 0 ? 'text-qb-negative' : 'text-qb-ink'}`}>{formatCents(account.balanceCents)}</span>
    </span>
  );
}
