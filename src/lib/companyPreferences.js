// Company preferences for screens that change behaviour by them (Preferences › Company
// Preferences). The values are kept in the company file (`src-tauri/src/preferences.rs`, with
// defaults in `preferences::DEFAULTS`); this reads them and keeps them current.

import { useEffect } from 'react';
import { api, useLedgerQuery } from './api.js';
import { periodRange } from './dates.js';

/** The company preferences, or null until they load. */
export function useCompanyPreferences() {
  return useLedgerQuery(api.getPreferences).data;
}

/**
 * The preferences and whether they are settled: `ready` once they loaded or failed to load, so a
 * screen can wait for a default without waiting forever.
 */
export function useCompanyPreferencesState() {
  const { data, error } = useLedgerQuery(api.getPreferences);
  return { prefs: data, ready: data != null || error != null };
}

/** `ACCRUAL` or `CASH`: the basis summary reports open on (Reports & Graphs). */
export const reportBasis = (prefs) => (prefs?.report_basis === 'cash' ? 'CASH' : 'ACCRUAL');

/** An account's name in a report, with its number and full path as Preferences › Accounting says. */
export function accountLabel(prefs, number, fullName, separator = ' · ') {
  const name = prefs?.lowest_subaccount_only === 'true' ? fullName.split(':').pop() : fullName;
  return number && prefs?.show_account_numbers !== 'false' ? `${number}${separator}${name}` : name;
}

/**
 * Send Forms › default delivery method: when it is email, a new invoice or estimate for a customer
 * with an email address starts with Email later ticked (and unticked for one without).
 */
export function useEmailLaterDefault(customerName, customers, setEmailLater, editing = false) {
  const prefs = useCompanyPreferences();
  const byEmail = prefs?.default_delivery_method === 'email';
  const customer = (customers ?? []).find((c) => c.name.toLowerCase() === String(customerName ?? '').trim().toLowerCase());
  useEffect(() => {
    if (byEmail && !editing) setEmailLater(Boolean(customer?.email));
  }, [byEmail, customer?.id, editing]); // eslint-disable-line react-hooks/exhaustive-deps
}

/** The sales tax period that ended most recently for the filing frequency (Preferences › Sales Tax). */
export function filingPeriod(prefs, today) {
  const id = { monthly: 'lastMonth', annually: 'lastYear' }[prefs?.sales_tax_filing_frequency] ?? 'lastQuarter';
  return periodRange(id, today);
}
