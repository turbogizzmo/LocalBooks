import { useMemo } from 'react';
import { formatCents } from '../lib/money.js';

const CLASS_ORDER = ['ASSET', 'LIABILITY', 'EQUITY', 'INCOME', 'EXPENSE'];
const CLASS_LABEL = {
  ASSET: 'Asset',
  LIABILITY: 'Liability',
  EQUITY: 'Equity',
  INCOME: 'Income',
  EXPENSE: 'Expense',
};

export default function ChartOfAccounts({ accounts, title = 'Chart of Accounts', subtitle = 'Review every active account, grouped by class and account type.' }) {
  const groups = useMemo(() => {
    const byClass = new Map();
    for (const account of accounts) {
      if (!account.isActive) continue;
      const cls = account.classification || 'OTHER';
      const bucket = byClass.get(cls) ?? { className: cls, label: CLASS_LABEL[cls] ?? cls, rows: [] };
      bucket.rows.push(account);
      byClass.set(cls, bucket);
    }

    return CLASS_ORDER.map((key) => byClass.get(key)).filter(Boolean).map((group) => ({
      ...group,
      rows: [...group.rows].sort((a, b) => {
        const typeCompare = (a.accountTypeName || '').localeCompare(b.accountTypeName || '');
        if (typeCompare !== 0) return typeCompare;
        const numberA = a.accountNumber ?? '';
        const numberB = b.accountNumber ?? '';
        return numberA.localeCompare(numberB) || (a.fullName || '').localeCompare(b.fullName || '');
      }),
      totalCents: group.rows.reduce((sum, row) => sum + (row.balanceCents ?? 0), 0),
    }));
  }, [accounts]);

  const activeCount = accounts.filter((account) => account.isActive).length;

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <section className="qb-panel flex min-h-0 flex-col">
        <div className="flex h-row-lg flex-none items-center justify-between border-b border-qb-border px-2">
          <div>
            <h2 className="text-md font-semibold">{title}</h2>
            <div className="text-xs text-qb-ink-muted">{subtitle}</div>
          </div>
          <span className="rounded-xs bg-qb-green-tint px-1.5 py-0.5 text-xs font-semibold text-qb-green-active">{activeCount} active accounts</span>
        </div>

        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          {groups.length === 0 ? (
            <div className="px-2 py-2 text-sm text-qb-ink-muted">No active accounts are in this file yet.</div>
          ) : (
            <div className="divide-y divide-qb-border">
              {groups.map((group) => (
                <section key={group.className} className="bg-white">
                  <div className="flex items-center justify-between border-b border-qb-border bg-qb-row-header px-2 py-1">
                    <div>
                      <div className="text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Class</div>
                      <div className="font-semibold text-qb-ink">{group.label}</div>
                    </div>
                    <div className="num text-sm font-semibold text-qb-ink">{formatCents(group.totalCents)}</div>
                  </div>

                  <table className="qb-grid w-full">
                    <colgroup>
                      <col className="w-[92px]" />
                      <col className="w-[200px]" />
                      <col />
                      <col className="w-[160px]" />
                    </colgroup>
                    <thead>
                      <tr>
                        <th className="qb-grid-head">No.</th>
                        <th className="qb-grid-head">Type</th>
                        <th className="qb-grid-head">Account</th>
                        <th className="qb-grid-head text-right">Balance</th>
                      </tr>
                    </thead>
                    <tbody>
                      {group.rows.map((account) => (
                        <tr key={account.id} className="qb-grid-row">
                          <td className="qb-grid-cell text-qb-ink-muted">{account.accountNumber ?? '—'}</td>
                          <td className="qb-grid-cell text-qb-ink-muted">{account.accountTypeName}</td>
                          <td className="qb-grid-cell" title={account.description ?? account.fullName}>{account.fullName}</td>
                          <td className="qb-grid-cell num font-semibold">{formatCents(account.balanceCents ?? 0)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </section>
              ))}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
