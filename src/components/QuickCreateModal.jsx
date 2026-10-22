import { useEffect, useRef } from 'react';
import { Icon } from '../lib/icons.js';

/** The classic Accountant "+ New" matrix: each item routes to a view and opens its entry form. */
export const QUICK_CREATE = [
  {
    heading: 'Customers',
    icon: 'Sales',
    items: [
      { id: 'invoice', label: 'Invoice', view: 'sales/invoices' },
      { id: 'receive-payment', label: 'Receive Payment', view: 'sales/invoices' },
      { id: 'estimate', label: 'Estimate', view: 'sales/estimates' },
      { id: 'sales-receipt', label: 'Sales Receipt', view: 'sales/all' },
    ],
  },
  {
    heading: 'Vendors',
    icon: 'Expenses',
    items: [
      { id: 'expense', label: 'Expense', view: 'expenses/expenses' },
      { id: 'check', label: 'Check', view: 'expenses/expenses' },
      { id: 'bill', label: 'Bill', view: 'expenses/bills' },
      { id: 'pay-bills', label: 'Pay Bills', view: 'expenses/bills' },
    ],
  },
  {
    heading: 'Employees',
    icon: 'Employees',
    items: [
      { id: 'time-activity', label: 'Single Time Activity', view: 'employees/time' },
      { id: 'weekly-timesheet', label: 'Weekly Timesheet', view: 'employees/time' },
    ],
  },
  {
    heading: 'Other',
    icon: 'Accounting',
    items: [
      { id: 'journal-entry', label: 'Journal Entry', view: 'accounting/journal' },
      { id: 'bank-deposit', label: 'Bank Deposit', view: 'banking/transfers' },
      { id: 'transfer', label: 'Transfer', view: 'banking/transfers' },
      { id: 'new-account', label: 'Account', view: 'accounting/chart-of-accounts' },
    ],
  },
];

export default function QuickCreateModal({ onClose, onCreate }) {
  const panelRef = useRef(null);

  useEffect(() => {
    const buttons = () => [...panelRef.current.querySelectorAll('button[data-item]')];
    buttons()[0]?.focus();
    const onKey = (e) => {
      if (e.key === 'Escape') {
        onClose();
        return;
      }
      // Arrow keys walk the grid; Tab stays inside the dialog.
      const list = buttons();
      const index = list.indexOf(document.activeElement);
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const next = index + (e.key === 'ArrowDown' ? 1 : -1);
        list[(next + list.length) % list.length]?.focus();
      } else if (e.key === 'Tab') {
        const all = [...panelRef.current.querySelectorAll('button')];
        const at = all.indexOf(document.activeElement);
        const nextAt = e.shiftKey ? (at <= 0 ? all.length - 1 : at - 1) : (at + 1) % all.length;
        e.preventDefault();
        all[nextAt].focus();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-qb-nav/30 pt-[12vh] backdrop-blur-sm" onMouseDown={onClose}>
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label="Create new"
        className="w-[760px] rounded-xs border border-qb-border-strong bg-white shadow-popover"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center border-b border-qb-border px-3 py-1">
          <h2 className="text-md font-semibold">New</h2>
          <span className="ml-2 text-xs text-qb-ink-muted">↑↓ to move · Enter to open · Esc to close</span>
          <button type="button" onClick={onClose} className="ml-auto px-1 text-lg leading-none text-qb-ink-muted hover:text-qb-ink" aria-label="Close">
            ×
          </button>
        </div>
        <div className="grid grid-cols-4 divide-x divide-qb-border px-1 py-2">
          {QUICK_CREATE.map((col) => (
            <section key={col.heading} className="px-2">
              <h3 className="mb-1 flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
                <Icon name={col.icon} />
                {col.heading}
              </h3>
              <ul>
                {col.items.map((item) => (
                  <li key={item.id}>
                    <button
                      type="button"
                      data-item={item.id}
                      onClick={() => onCreate(item)}
                      className="block w-full rounded-xs px-1.5 py-0.5 text-left text-base hover:bg-qb-green-tint hover:text-qb-green-active focus:bg-qb-green-tint focus:text-qb-green-active focus:outline-none"
                    >
                      {item.label}
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}
