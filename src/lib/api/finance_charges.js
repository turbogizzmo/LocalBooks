// Finance charges on overdue invoices.
import { invoke } from '@tauri-apps/api/core';

export default {
  previewFinanceCharges: (asOf) => invoke('preview_finance_charges', { asOf }),
  assessFinanceCharges: (input) => invoke('assess_finance_charges', { input }),
};
