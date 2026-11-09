// Reconciliation.
import { invoke } from '@tauri-apps/api/core';

export default {
  getReconcileState: (accountId) => invoke('get_reconcile_state', { accountId }),
  getUnreconciledLines: (accountId) => invoke('get_unreconciled_lines', { accountId }),
  listReconciliations: (accountId) => invoke('list_reconciliations', { accountId }),
  // `charges`: the statement's service charge and interest earned (reconcileCharges.js), posted with it.
  startReconciliation: (accountId, statementEndDate, statementEndingBalanceCents, charges = []) =>
    invoke('start_reconciliation', { accountId, statementEndDate, statementEndingBalanceCents, charges }),
  setLinesCleared: (reconciliationId, lineIds, cleared) =>
    invoke('set_lines_cleared', { reconciliationId, lineIds, cleared }),
  lockReconciliation: (reconciliationId) => invoke('lock_reconciliation', { reconciliationId }),
  cancelReconciliation: (reconciliationId) => invoke('cancel_reconciliation', { reconciliationId }),
  // Undo Last Reconciliation; resolves how many transactions were uncleared.
  undoLastReconciliation: (reconciliationId, reason = null) => invoke('undo_last_reconciliation', { reconciliationId, reason }),
};
