// Estimates and turning them into invoices.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  getEstimateDetail: (estimateId) => invoke('get_estimate_detail', { estimateId }),
  invoiceFromEstimate: (estimateId, input) => recordTransaction('invoice_from_estimate', { estimateId, input }),
  listEstimates: () => invoke('list_estimates'),
  createEstimate: (input) => recordTransaction('create_estimate', { input }),
  setEstimateStatus: (estimateId, status) => invoke('set_estimate_status', { estimateId, status }),
  convertEstimateToInvoice: (estimateId, issueDate, dueDate) =>
    invoke('convert_estimate_to_invoice', { estimateId, issueDate, dueDate }),
};
