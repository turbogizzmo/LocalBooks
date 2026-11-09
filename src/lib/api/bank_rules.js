// Bank feed rules: renaming rules for downloaded payees, and the learned category rules' list.
import { invoke } from '@tauri-apps/api/core';

export default {
  listBankPayeeRules: () => invoke('list_bank_payee_rules'),
  saveBankPayeeRule: (rule) => invoke('save_bank_payee_rule', { rule }),
  removeBankPayeeRule: (id) => invoke('remove_bank_payee_rule', { id }),
  listBankCategoryRules: () => invoke('list_bank_category_rules'),
  setBankCategoryRuleAccount: (id, accountId) => invoke('set_bank_category_rule_account', { id, accountId }),
  removeBankCategoryRule: (id) => invoke('remove_bank_category_rule', { id }),
};
