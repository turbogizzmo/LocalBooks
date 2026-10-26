// Foreign currency revaluation and exchange-rate lookup.
import { invoke } from '@tauri-apps/api/core';

export default {
  previewFxRevaluation: (asOf, rates) => invoke('preview_fx_revaluation', { asOf, rates }),
  postFxRevaluation: (asOf, rates) => invoke('post_fx_revaluation', { asOf, rates }),
  listFxRevaluations: () => invoke('list_fx_revaluations'),
  voidFxRevaluation: (id) => invoke('void_fx_revaluation', { id }),
  lookupExchangeRate: (currency, date) => invoke('lookup_exchange_rate', { currency, date }),
};
