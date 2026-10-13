import { useState } from 'react';

const icon = (d) => (
  <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
    <path d={d} strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

/** Navigation tree: top-level links and accordion groups, QuickBooks Online classic layout. */
export const NAV = [
  { id: 'dashboard', label: 'Dashboard', icon: icon('M2 14V8M6 14V4M10 14V9M14 14V2') },
  {
    id: 'sales',
    label: 'Sales',
    icon: icon('M2 12l4-4 3 3 5-6M10 5h4v4'),
    children: [
      { id: 'sales/all', label: 'All Sales' },
      { id: 'sales/invoices', label: 'Invoices' },
      { id: 'sales/estimates', label: 'Estimates' },
      { id: 'sales/customers', label: 'Customers' },
      { id: 'sales/products', label: 'Products & Services' },
    ],
  },
  {
    id: 'expenses',
    label: 'Expenses',
    icon: icon('M4 1.5h8v13l-2-1.2-2 1.2-2-1.2-2 1.2zM6 5h4M6 8h4'),
    children: [
      { id: 'expenses/expenses', label: 'Expenses & Checks' },
      { id: 'expenses/bills', label: 'Bills' },
      { id: 'expenses/vendors', label: 'Vendors' },
    ],
  },
  {
    id: 'employees',
    label: 'Employees',
    icon: icon('M8 7.5a2.5 2.5 0 100-5 2.5 2.5 0 000 5zM3 14c.5-2.8 2.5-4.5 5-4.5s4.5 1.7 5 4.5'),
    children: [{ id: 'employees/time', label: 'Time Tracking' }],
  },
  {
    id: 'banking',
    label: 'Banking',
    icon: icon('M1.5 6L8 2l6.5 4M3 6.5v6M6.3 6.5v6M9.7 6.5v6M13 6.5v6M1.5 14.5h13'),
    children: [
      { id: 'banking/feeds', label: 'Bank Feeds' },
      { id: 'banking/transfers', label: 'Deposits & Transfers' },
      { id: 'banking/reconcile', label: 'Reconcile' },
    ],
  },
  {
    id: 'accounting',
    label: 'Accounting',
    icon: icon('M3 2.5h10v11H3zM5.5 5.5h5M5.5 8h5M5.5 10.5h3'),
    children: [
      { id: 'accounting/journal', label: 'Journal Entries' },
      { id: 'accounting/audit', label: 'Audit Log' },
      { id: 'accounting/qbw-import', label: 'QuickBooks Import' },
    ],
  },
  { id: 'reports', label: 'Reports', icon: icon('M4 1.5h6l3 3v10H4zM10 1.5v3h3M6 12V9M8.5 12V7M11 12v-2') },
  { id: 'settings', label: 'Settings', icon: icon('M8 10a2 2 0 100-4 2 2 0 000 4zM8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4') },
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
          <span className="text-md leading-none">＋</span> New
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
