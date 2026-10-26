// Reconciliation.
import { invoke } from '@tauri-apps/api/core';

export default {
  getReconcileState: (accountId) => invoke('get_reconcile_state', { accountId }),
  getUnreconciledLines: (accountId) => invoke('get_unreconciled_lines', { accountId }),
  listReconciliations: (accountId) => invoke('list_reconciliations', { accountId }),
  startReconciliation: (accountId, statementEndDate, statementEndingBalanceCents) =>
    invoke('start_reconciliation', { accountId, statementEndDate, statementEndingBalanceCents }),
  setLinesCleared: (reconciliationId, lineIds, cleared) =>
    invoke('set_lines_cleared', { reconciliationId, lineIds, cleared }),
  lockReconciliation: (reconciliationId) => invoke('lock_reconciliation', { reconciliationId }),
  cancelReconciliation: (reconciliationId) => invoke('cancel_reconciliation', { reconciliationId }),
};
