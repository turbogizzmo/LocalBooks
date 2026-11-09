// Bank feed matching (downloaded lines to transactions already entered) and Add several at once.
import { invoke } from '@tauri-apps/api/core';

export default {
  listBankMatchCandidates: (transactionId) => invoke('list_bank_match_candidates', { transactionId }),
  matchBankTransaction: (transactionId, journalEntryId) => invoke('match_bank_transaction', { transactionId, journalEntryId }),
  unmatchBankTransaction: (transactionId) => invoke('unmatch_bank_transaction', { transactionId }),
  listMatchedBankTransactions: () => invoke('list_matched_bank_transactions'),
  approveBankTransactions: (lines) => invoke('approve_bank_transactions', { lines }),
};
