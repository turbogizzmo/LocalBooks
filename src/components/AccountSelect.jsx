import { forwardRef } from 'react';

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
