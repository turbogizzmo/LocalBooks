// Deposits and transfers.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  listUndeposited: () => invoke('list_undeposited'),
  postBankDeposit: (input) => recordTransaction('post_bank_deposit', { input }),
  postTransfer: (input) => recordTransaction('post_transfer', { input }),
  listBankingActivity: () => invoke('list_banking_activity'),
};
