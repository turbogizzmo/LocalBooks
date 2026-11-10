// The money bars' extra segments (unbilled, estimates, purchase orders, card balances).
import { invoke } from '@tauri-apps/api/core';

export default {
  getMoneyBarSegments: (side) => invoke('get_money_bar_segments', { side }),
};
