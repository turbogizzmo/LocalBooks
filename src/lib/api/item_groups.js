// Item groups: a named set of items a sales form expands into its lines.
import { invoke } from '@tauri-apps/api/core';

export default {
  listItemGroups: () => invoke('list_item_groups'),
  // `id` null adds a group. input: { name, description, printItems, members: [{ productId, quantityE4 }] }
  saveItemGroup: (id, input) => invoke('save_item_group', { id, input }),
  setItemGroupActive: (id, active) => invoke('set_item_group_active', { id, active }),
};
