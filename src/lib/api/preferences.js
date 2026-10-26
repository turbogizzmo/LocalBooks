// Preferences, tax calendar and closing date.
import { invoke } from '@tauri-apps/api/core';

export default {
  getPreferences: () => invoke('get_preferences'),
  setPreference: (key, value) => invoke('set_preference', { key, value }),
  listTaxForms: () => invoke('list_tax_forms'),
  getFiscalPeriods: () => invoke('get_fiscal_periods', { asOf: null }),
  getTaxCalendar: () => invoke('get_tax_calendar', { asOf: null }),
  getClosingStatus: () => invoke('get_closing_status'),
  setClosingDate: (input) => invoke('set_closing_date', { input }),
};
