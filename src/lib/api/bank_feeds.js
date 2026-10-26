// Bank file import, SimpleFIN connections and the bank feed.
import { invoke } from '@tauri-apps/api/core';

export default {
  importBankFile: (path, bankAccountId, mapping = null) => invoke('import_bank_csv', { path, bankAccountId, mapping }),
  inspectBankFile: (path, mapping = null, bankAccountId = null) => invoke('inspect_bank_file', { path, mapping, bankAccountId }),
  listBankTransactions: () => invoke('list_bank_transactions'),
  approveBankTransaction: (transactionId, categoryAccountId, classId = null, payeeName = null) =>
    invoke('approve_bank_transaction', { transactionId, categoryAccountId, classId, payeeName }),
  excludeBankTransaction: (transactionId) => invoke('exclude_bank_transaction', { transactionId }),
  listBankConnections: () => invoke('simplefin_list'),
  connectBank: (token) => invoke('simplefin_connect', { token }),
  reconnectBank: (connectionId, token) => invoke('simplefin_reconnect', { connectionId, token }),
  refreshBankAccounts: (connectionId) => invoke('simplefin_refresh_accounts', { connectionId }),
  linkBankAccount: (accountId, bankAccountId, syncFrom) => invoke('simplefin_link', { accountId, bankAccountId, syncFrom }),
  unlinkBankAccount: (accountId) => invoke('simplefin_unlink', { accountId }),
  suggestedBankStart: (bankAccountId) => invoke('simplefin_suggested_start', { bankAccountId }),
  syncBankConnection: (connectionId) => invoke('simplefin_sync', { connectionId }),
  disconnectBank: (connectionId) => invoke('simplefin_disconnect', { connectionId }),
};
