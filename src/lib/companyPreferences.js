// Company preferences for screens that change behaviour by them (Preferences › Company
// Preferences). The values are kept in the company file (`src-tauri/src/preferences.rs`, with
// defaults in `preferences::DEFAULTS`); this reads them and keeps them current.

import { api, useLedgerQuery } from './api.js';

/** The company preferences, or null until they load. */
export function useCompanyPreferences() {
  return useLedgerQuery(api.getPreferences).data;
}

/** `ACCRUAL` or `CASH`: the basis summary reports open on (Reports & Graphs). */
export const reportBasis = (prefs) => (prefs?.report_basis === 'cash' ? 'CASH' : 'ACCRUAL');

/** An account's name in a report, with its number and full path as Preferences › Accounting says. */
export function accountLabel(prefs, number, fullName, separator = ' · ') {
  const name = prefs?.lowest_subaccount_only === 'true' ? fullName.split(':').pop() : fullName;
  return number && prefs?.show_account_numbers !== 'false' ? `${number}${separator}${name}` : name;
}
