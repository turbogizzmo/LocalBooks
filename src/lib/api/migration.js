// Desktop Migration Wizard: importing other programs' books through a staging sandbox.
import { invoke } from '@tauri-apps/api/core';

export default {
  ingestRawQbw: (filePath) => invoke('ingest_raw_qbw', { filePath }),
  ingestReportExports: (filePaths) => invoke('ingest_report_exports', { filePaths }),
  listQbwBatches: () => invoke('list_qbw_batches'),
  getQbwBatch: (batchId) => invoke('get_qbw_batch', { batchId }),
  setQbwMapping: (batchId, rawLabel, accountId) => invoke('set_qbw_mapping', { batchId, rawLabel, accountId }),
  autoCreateQbwAccounts: (batchId) => invoke('auto_create_qbw_accounts', { batchId }),
  postQbwBatch: (input) => invoke('post_qbw_batch', { input }),
  discardQbwBatch: (batchId) => invoke('discard_qbw_batch', { batchId }),
  reverseQbwBatch: (batchId) => invoke('reverse_qbw_batch', { batchId }),
  listLedgerBatches: () => invoke('list_ledger_batches'),
  getLedgerBatch: (batchId) => invoke('get_ledger_batch', { batchId }),
  setLedgerAccount: (batchId, sourceId, accountId) => invoke('set_ledger_account', { batchId, sourceId, accountId }),
  autoMapLedgerAccounts: (batchId) => invoke('auto_map_ledger_accounts', { batchId }),
  importLedgerLists: (batchId) => invoke('import_ledger_lists', { batchId }),
  postLedgerBatch: (input) => invoke('post_ledger_batch', { input }),
  reverseLedgerBatch: (batchId) => invoke('reverse_ledger_batch', { batchId }),
  discardLedgerBatch: (batchId) => invoke('discard_ledger_batch', { batchId }),
  startOverLedgerBatch: (batchId) => invoke('start_over_ledger_batch', { batchId }),
};
