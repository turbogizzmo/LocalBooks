// Paying bills by NACHA ACH file: vendor bank details, originator details and ACH files.
import { invoke } from '@tauri-apps/api/core';

export default {
  getVendorBank: (vendorId) => invoke('get_vendor_bank', { vendorId }),
  setVendorBank: (vendorId, input) => invoke('set_vendor_bank', { vendorId, input }),
  getAchOriginator: (accountId) => invoke('get_ach_originator', { accountId }),
  setAchOriginator: (input) => invoke('set_ach_originator', { input }),
  payBillsByAch: (input, path) => invoke('pay_bills_by_ach', { input, path }),
  saveAchFile: (id, path) => invoke('save_ach_file', { id, path }),
  listAchFiles: () => invoke('list_ach_files'),
};
