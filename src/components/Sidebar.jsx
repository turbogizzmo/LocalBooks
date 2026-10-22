import { useState } from 'react';

import { Icon } from '../lib/icons.js';

/** Navigation tree: top-level links and accordion groups, in a classic accounting-software layout. */
export const NAV = [
  { id: 'dashboard', label: 'Dashboard', icon: <Icon name="Dashboard" /> },
  {
    id: 'sales',
    label: 'Sales',
    icon: <Icon name="Sales" />,
    children: [
      { id: 'sales/all', label: 'All Sales' },
      { id: 'sales/invoices', label: 'Invoices' },
      { id: 'sales/estimates', label: 'Estimates' },
      { id: 'sales/credits', label: 'Credit Memos & Refunds' },
      { id: 'sales/customers', label: 'Customers' },
      { id: 'sales/products', label: 'Products & Services' },
    ],
  },
  {
    id: 'expenses',
    label: 'Expenses',
    icon: <Icon name="Expenses" />,
    children: [
      { id: 'expenses/expenses', label: 'Expenses & Checks' },
      { id: 'expenses/bills', label: 'Bills' },
      { id: 'expenses/credits', label: 'Vendor Credits' },
      { id: 'expenses/vendors', label: 'Vendors' },
    ],
  },
  {
    id: 'employees',
    label: 'Employees',
    icon: <Icon name="Employees" />,
    children: [{ id: 'employees/time', label: 'Time Tracking' }],
  },
  {
    id: 'banking',
    label: 'Banking',
    icon: <Icon name="Banking" />,
    children: [
      { id: 'banking/feeds', label: 'Bank Feeds' },
      { id: 'banking/transfers', label: 'Deposits & Transfers' },
      { id: 'banking/reconcile', label: 'Reconcile' },
    ],
  },
  {
    id: 'accounting',
    label: 'Accounting',
    icon: <Icon name="Accounting" />,
    children: [
      { id: 'accounting/chart-of-accounts', label: 'Chart of Accounts' },
      { id: 'accounting/register', label: 'Account Registers' },
      { id: 'accounting/classes', label: 'Classes' },
      { id: 'accounting/reclassify', label: 'Reclassify Transactions' },
      { id: 'accounting/journal', label: 'Journal Entries' },
      { id: 'accounting/audit', label: 'Audit Log' },
      { id: 'accounting/fx-revaluation', label: 'Currency Revaluation' },
      { id: 'accounting/qbw-import', label: 'Desktop Migration Wizard' },
    ],
  },
  { id: 'reports', label: 'Reports', icon: <Icon name="Reports" /> },
  { id: 'settings', label: 'Settings', icon: <Icon name="Settings" /> },
];

export function findNav(id) {
  for (const item of NAV) {
    if (item.id === id) return { item, parent: null };
    const child = item.children?.find((c) => c.id === id);
    if (child) return { item: child, parent: item };
  }
  return null;
}

export default function Sidebar({ current, onSelect, onNew, companyName, storage }) {
  const [open, setOpen] = useState(() => new Set(NAV.filter((n) => n.children).map((n) => n.id)));
  const toggle = (id) =>
    setOpen((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <nav className="qb-sidebar no-print flex flex-none flex-col" aria-label="Main">
      <div className="flex h-toolbar items-center gap-2 border-b border-white/10 px-3">
        <span className="grid h-5 w-5 place-items-center rounded-xs bg-qb-green text-2xs font-bold text-white">LB</span>
        <span className="truncate text-md font-semibold text-white" title={companyName}>{companyName}</span>
      </div>
      <div className="px-2 py-1.5">
        <button type="button" onClick={onNew} className="qb-btn w-full justify-start gap-1.5 rounded-full border-white/40 bg-transparent text-white hover:bg-white/10" aria-haspopup="dialog">
          <Icon name="NewTransaction" /> New
        </button>
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto py-1 scrollbar-thin">
        {NAV.map((item) =>
          item.children ? (
            <li key={item.id}>
              <button
                type="button"
                className="qb-nav-item w-full text-left"
                aria-expanded={open.has(item.id)}
                onClick={() => toggle(item.id)}
              >
                {item.icon}
                <span className="flex-1">{item.label}</span>
                <svg
                  viewBox="0 0 10 10"
                  width="8"
                  height="8"
                  className={`transition-transform duration-75 ${open.has(item.id) ? 'rotate-90' : ''}`}
                  aria-hidden="true"
                >
                  <path d="M3 1.5L6.5 5 3 8.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
                </svg>
              </button>
              {open.has(item.id) && (
                <ul>
                  {item.children.map((child) => (
                    <li key={child.id}>
                      <button
                        type="button"
                        className="qb-nav-item h-row w-full pl-[34px] text-left text-sm"
                        aria-current={child.id === current ? 'page' : undefined}
                        onClick={() => onSelect(child.id)}
                      >
                        {child.label}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </li>
          ) : (
            <li key={item.id}>
              <button
                type="button"
                className="qb-nav-item w-full text-left"
                aria-current={item.id === current ? 'page' : undefined}
                onClick={() => onSelect(item.id)}
              >
                {item.icon}
                {item.label}
              </button>
            </li>
          ),
        )}
      </ul>
      <div className="truncate border-t border-white/10 px-3 py-1 text-2xs text-qb-nav-text" title={storage?.detail}>
        {storage ? `${storage.kind === 'LOCAL' ? '●' : '▲'} ${storage.detail} · offline` : 'Company file · offline'}
      </div>
    </nav>
  );
}
