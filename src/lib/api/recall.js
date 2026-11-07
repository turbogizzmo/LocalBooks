// Recalling a vendor's last transaction (My Preferences › General).
import { invoke } from '@tauri-apps/api/core';

export default {
  /** The lines of the vendor's last live check, expense or bill: `{kind, date, lines}` or null. */
  recallVendorTransaction: (vendorName) => invoke('recall_vendor_transaction', { vendorName }),
};
