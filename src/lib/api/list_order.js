// Lists in a custom order: list is 'ACCOUNTS', 'ITEMS' or 'CUSTOMERS'; ids first to last.
import { invoke } from '@tauri-apps/api/core';

export default {
  getListOrder: (list) => invoke('get_list_order', { list }),
  setListOrder: (list, ids) => invoke('set_list_order', { list, ids }),
  resetListOrder: (list) => invoke('reset_list_order', { list }),
};
