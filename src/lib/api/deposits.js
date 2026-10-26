// Deposits and transfers.
import { invoke } from '@tauri-apps/api/core';

export default {
  listUndeposited: () => invoke('list_undeposited'),
  postBankDeposit: (input) => invoke('post_bank_deposit', { input }),
  postTransfer: (input) => invoke('post_transfer', { input }),
  listBankingActivity: () => invoke('list_banking_activity'),
};
