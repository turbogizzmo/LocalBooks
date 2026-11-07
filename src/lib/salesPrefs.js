// Preferences batch 4 as the screens read them: the company's sales, payments, bills and Send
// Forms defaults (keys in `src-tauri/src/sales_prefs.rs`, defaults in `preferences::DEFAULTS`).
// Until the preferences load, each option reads as its default.

import { useEffect } from 'react';
import { api, useLedgerQuery } from './api.js';

export const JOB_STATUSES = [
  { code: 'PENDING', key: 'job_status_pending', label: 'Pending' },
  { code: 'AWARDED', key: 'job_status_awarded', label: 'Awarded' },
  { code: 'IN_PROGRESS', key: 'job_status_in_progress', label: 'In progress' },
  { code: 'CLOSED', key: 'job_status_closed', label: 'Closed' },
  { code: 'NOT_AWARDED', key: 'job_status_not_awarded', label: 'Not awarded' },
];

/** The batch 4 options from a preferences map (or `null` while loading). */
export function salesPrefsOf(prefs) {
  const p = prefs ?? {};
  const shipId = p.default_ship_method_id ? Number(p.default_ship_method_id) : null;
  return {
    loaded: Boolean(prefs),
    priceLevels: p.price_levels_enabled !== 'false',
    estimates: p.estimates_enabled !== 'false',
    defaultShipMethodId: Number.isFinite(shipId) ? shipId : null,
    defaultFob: p.default_fob ?? '',
    defaultMarkup: p.default_markup_percent ?? '0',
    dropZeroLines: p.estimate_drop_zero_lines !== 'false',
    autoApply: p.payment_auto_apply ?? 'exact',
    autoCalculate: p.payment_auto_calculate !== 'false',
    undepositedDefault: p.payment_undeposited_default !== 'false',
    autoCredits: p.bill_auto_apply_credits === 'true',
    autoDiscounts: p.bill_auto_apply_discounts === 'true' && Boolean(p.bill_discount_account_id),
    jobStatus: Object.fromEntries(JOB_STATUSES.map((s) => [s.code, p[s.key] || s.label])),
  };
}

/** Screens a company preference turned off, hidden from the side bar and the New menu. */
export function hiddenViews(sp) {
  return new Set(sp.estimates ? [] : ['sales/estimates']);
}

export function useSalesPrefs() {
  return salesPrefsOf(useLedgerQuery(api.getPreferences).data);
}

/**
 * Where a received payment goes by default: Undeposited Funds, or (with that preference off) the
 * first active bank account.
 */
export function defaultDepositAccount(accounts, undepositedDefault) {
  const undeposited = accounts.find((a) => a.isSystem && a.accountType === 'OTHER_CURRENT_ASSET');
  const bank = accounts.find((a) => a.isActive !== false && a.accountType === 'BANK');
  return (undepositedDefault ? (undeposited ?? bank) : (bank ?? undeposited))?.id ?? null;
}

/** Whether a customer's new forms start ticked to email or print later. */
export function deliveryOf(customer) {
  // Email later only helps for a customer there is an address to send to.
  return { emailLater: customer?.preferredDelivery === 'EMAIL' && Boolean(customer?.email?.trim()), printLater: customer?.preferredDelivery === 'PRINT' };
}

/**
 * On a new form, ticks Email later or Print later from the customer's preferred delivery
 * (Customer › Preferred delivery; new customers take Preferences › Send Forms' default) whenever
 * the customer changes. A customer without one leaves the boxes as they are.
 */
export function useCustomerDelivery(customerName, customers, { enabled = true, setEmailLater, printLater = null }) {
  const name = (customerName ?? '').trim().toLowerCase();
  const row = (customers ?? []).find((c) => c.name.toLowerCase() === name);
  const preferred = row?.preferredDelivery ?? null;
  useEffect(() => {
    if (!enabled || !preferred) return;
    const { emailLater, printLater: print } = deliveryOf(row);
    setEmailLater(emailLater);
    printLater?.follow(print);
    // Only a different customer (or their changed preference) re-ticks the boxes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, row?.id, preferred]);
}
