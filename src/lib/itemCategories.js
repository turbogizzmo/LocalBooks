// Item categories: which items a category covers (itself and its sub-categories), and the counts
// beside the category tree. `null` is Uncategorized; `undefined` means every item.

export const UNCATEGORIZED = 'none';

/** Builds lookups over `categories` (as list_item_categories returns them). */
export function categoryIndex(categories) {
  const children = new Map();
  for (const c of categories) {
    const key = c.parentId ?? null;
    if (!children.has(key)) children.set(key, []);
    children.get(key).push(c.id);
  }
  const subtree = new Map();
  const collect = (id, seen = new Set()) => {
    if (subtree.has(id)) return subtree.get(id);
    const all = new Set([id]);
    seen.add(id);
    for (const child of children.get(id) ?? []) {
      if (!seen.has(child)) for (const x of collect(child, seen)) all.add(x);
    }
    subtree.set(id, all);
    return all;
  };
  return {
    /** Whether an item in `itemCategory` shows under the chosen category (`undefined`: all). */
    covers(chosen, itemCategory) {
      if (chosen === undefined) return true;
      if (chosen === null) return itemCategory === null || itemCategory === undefined;
      return itemCategory != null && collect(chosen).has(itemCategory);
    },
    /** Items under each category, counting its sub-categories, plus all and uncategorized. */
    counts(items) {
      const byCategory = new Map();
      for (const c of categories) byCategory.set(c.id, items.filter((i) => i.categoryId != null && collect(c.id).has(i.categoryId)).length);
      return { all: items.length, none: items.filter((i) => i.categoryId == null).length, byCategory };
    },
  };
}
