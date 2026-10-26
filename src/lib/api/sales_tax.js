// Sales tax.
import { invoke } from '@tauri-apps/api/core';

export default {
  listTaxRates: () => invoke('list_tax_rates'),
  createTaxRate: (input) => invoke('create_tax_rate', { input }),
  updateTaxRate: (id, input) => invoke('update_tax_rate', { id, input }),
  setTaxRateActive: (id, active) => invoke('set_tax_rate_active', { id, active }),
  paySalesTax: (input) => invoke('pay_sales_tax', { input }),
  listSalesTaxPayments: () => invoke('list_sales_tax_payments'),
  getTaxLiability: (startDate, endDate) => invoke('get_tax_liability', { startDate, endDate }),
};
