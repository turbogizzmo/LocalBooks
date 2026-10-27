// Accounting › Clean Up Books.
import { invoke } from '@tauri-apps/api/core';

export default {
  listUnappliedCredits: () => invoke('list_unapplied_credits'),
  applyCleanupCredits: (input) => invoke('apply_cleanup_credits', { input }),
  listUndepositedReview: () => invoke('list_undeposited_review'),
  matchUndepositedFunds: (items) => invoke('match_undeposited_funds', { items }),
  unmatchUndepositedFunds: (matchId) => invoke('unmatch_undeposited_funds', { matchId }),
  previewWriteOffs: (filter) => invoke('preview_write_offs', { filter }),
  writeOffInvoices: (input) => invoke('write_off_invoices', { input }),
  listListChanges: (startDate, endDate) => invoke('list_list_changes', { startDate, endDate }),
  previewInventoryComparison: (asOf) => invoke('preview_inventory_comparison', { asOf }),
};
