// Item categories: the Category List (with sub-categories) and moving items between categories.
import { invoke } from '@tauri-apps/api/core';

export default {
  listItemCategories: () => invoke('list_item_categories'),
  // input: { name, parentId? }
  createItemCategory: (input) => invoke('create_item_category', { input }),
  updateItemCategory: (id, input) => invoke('update_item_category', { id, input }),
  setItemCategoryActive: (id, active) => invoke('set_item_category_active', { id, active }),
  deleteItemCategory: (id) => invoke('delete_item_category', { id }),
  // categoryId null = Uncategorized
  moveItemsToCategory: (itemIds, categoryId) => invoke('move_items_to_category', { itemIds, categoryId }),
};
