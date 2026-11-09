// Form 1099-NEC: vendors' W-9 details, and the year's forms with their 1096 summary.
import { invoke } from '@tauri-apps/api/core';

export default {
  getVendorW9: (vendorId) => invoke('get_vendor_w9', { vendorId }),
  setVendorW9: (vendorId, input) => invoke('set_vendor_w9', { vendorId, input }),
  get1099NecSummary: (year) => invoke('get_1099_nec_summary', { year }),
};
