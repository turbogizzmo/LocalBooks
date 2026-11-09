import { useMemo, useState } from "react";
import RowMenu from "./RowMenu.jsx";
import { Dialog } from "./InventoryDialog.jsx";
import Notice from "./Notice.jsx";
import { api, errorText, useLedgerQuery } from "../lib/api.js";
import { askConfirm, askText } from "../lib/dialogs.jsx";
import { categoryIndex, UNCATEGORIZED } from "../lib/itemCategories.js";

/**
 * Item categories (Lists › Item Categories): the tree beside the item list, the category box on
 * an item, and the category filter on forms and item reports. Preferences › Items & Inventory
 * turns them off, which hides all of this.
 */

/** The categories and whether they are on; `index` answers which items a category covers. */
export function useItemCategories() {
  const prefs = useLedgerQuery(api.getPreferences).data;
  const { data } = useLedgerQuery(api.listItemCategories);
  const enabled = prefs ? prefs.item_categories !== "false" : false;
  const index = useMemo(() => categoryIndex(data ?? []), [data]);
  return { enabled, categories: data ?? [], index };
}

/** A category picker: `value` is a category id, null for Uncategorized. */
export function CategorySelect({
  categories,
  value,
  onChange,
  className = "qb-input",
  allLabel = null,
  ...props
}) {
  const current =
    allLabel !== null && value === undefined
      ? ""
      : value === null
        ? UNCATEGORIZED
        : String(value);
  return (
    <select
      className={className}
      value={current}
      onChange={(e) =>
        onChange(
          e.target.value === ""
            ? undefined
            : e.target.value === UNCATEGORIZED
              ? null
              : Number(e.target.value),
        )
      }
      {...props}
    >
      {allLabel !== null && <option value="">{allLabel}</option>}
      <option value={UNCATEGORIZED}>Uncategorized</option>
      {categories
        .filter((c) => c.isActive || c.id === value)
        .map((c) => (
          <option key={c.id} value={c.id}>
            {"  ".repeat(c.depth)}
            {c.name}
            {c.isActive ? "" : " (inactive)"}
          </option>
        ))}
    </select>
  );
}

/** The tree beside the item list: All items, Uncategorized and each category, with their counts. */
export function CategoryTree({
  categories,
  selected,
  onSelect,
  counts,
  onNotice,
}) {
  const [showInactive, setShowInactive] = useState(false);
  const run = async (work, text) => {
    try {
      await work();
      if (text) onNotice({ kind: "success", text });
    } catch (err) {
      onNotice({ kind: "error", text: errorText(err) });
    }
  };
  const add = async (parent) => {
    const name = await askText(
      parent ? `New sub-category of ${parent.fullName}:` : "New category:",
    );
    if (name?.trim())
      await run(
        () => api.createItemCategory({ name, parentId: parent?.id ?? null }),
        `Added the category ${name.trim()}.`,
      );
  };
  const rename = async (c) => {
    const name = await askText(`Rename ${c.fullName}:`, c.name);
    if (name?.trim() && name.trim() !== c.name)
      await run(
        () => api.updateItemCategory(c.id, { name, parentId: c.parentId }),
        `Renamed to ${name.trim()}.`,
      );
  };
  const remove = async (c) => {
    if (
      await askConfirm(
        `Delete the category ${c.fullName}? Only an empty category can be deleted.`,
        { confirmLabel: "Delete", danger: true },
      )
    ) {
      await run(async () => {
        await api.deleteItemCategory(c.id);
        if (selected === c.id) onSelect(undefined);
      }, `Deleted ${c.fullName}.`);
    }
  };
  const row = (
    key,
    label,
    value,
    depth,
    count,
    extra = null,
    inactive = false,
  ) => (
    <li
      key={key}
      className={`group flex items-center ${selected === value ? "bg-qb-row-selected font-semibold" : "hover:bg-qb-row-hover"} ${inactive ? "text-qb-ink-faint" : ""}`}
    >
      <button
        type="button"
        className="min-w-0 flex-1 truncate py-0.5 text-left"
        style={{ paddingLeft: 6 + depth * 12 }}
        onClick={() => onSelect(value)}
        aria-current={selected === value}
      >
        {label}
        {count > 0 && (
          <span className="ml-1 text-2xs text-qb-ink-muted">{count}</span>
        )}
      </button>
      {extra}
    </li>
  );
  return (
    <nav
      className="flex w-[210px] flex-none flex-col border-r border-qb-border text-sm"
      aria-label="Item categories"
    >
      <div className="flex h-row items-center gap-1 border-b border-qb-border px-1.5">
        <span className="text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
          Categories
        </span>
        <button
          type="button"
          className="ml-auto text-xs text-qb-link hover:underline"
          onClick={() => add(null)}
        >
          New…
        </button>
      </div>
      <ul className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        {row("all", "All items", undefined, 0, counts.all)}
        {row("none", "Uncategorized", null, 0, counts.none)}
        {categories
          .filter((c) => showInactive || c.isActive)
          .map((c) =>
            row(
              c.id,
              c.name,
              c.id,
              c.depth,
              counts.byCategory.get(c.id) ?? 0,
              <RowMenu
                label={`Actions for category ${c.fullName}`}
                items={[
                  {
                    label: "New sub-category…",
                    disabled: !c.isActive,
                    onSelect: () => add(c),
                  },
                  { label: "Rename…", onSelect: () => rename(c) },
                  c.isActive
                    ? {
                        label: "Make inactive",
                        onSelect: () =>
                          run(
                            () => api.setItemCategoryActive(c.id, false),
                            `${c.fullName} is inactive; its items keep it.`,
                          ),
                      }
                    : {
                        label: "Make active",
                        onSelect: () =>
                          run(
                            () => api.setItemCategoryActive(c.id, true),
                            `${c.fullName} is active again.`,
                          ),
                      },
                  { label: "Delete…", onSelect: () => remove(c) },
                ]}
              />,
              !c.isActive,
            ),
          )}
      </ul>
      <label className="flex items-center gap-1 border-t border-qb-border px-1.5 py-0.5 text-xs">
        <input
          type="checkbox"
          checked={showInactive}
          onChange={(e) => setShowInactive(e.target.checked)}
        />
        Show inactive
      </label>
    </nav>
  );
}

/** Move to a different category: one item or the ones ticked. */
export function MoveToCategoryDialog({ items, categories, onClose, onDone }) {
  const [target, setTarget] = useState(
    items.length === 1 ? (items[0].categoryId ?? null) : null,
  );
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const submit = async (e) => {
    e.preventDefault();
    setBusy(true);
    try {
      await api.moveItemsToCategory(
        items.map((i) => i.id),
        target,
      );
      const where =
        target === null
          ? "Uncategorized"
          : categories.find((c) => c.id === target)?.fullName;
      onDone(
        `Moved ${items.length === 1 ? items[0].name : `${items.length} items`} to ${where}.`,
      );
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };
  return (
    <Dialog
      title={
        items.length === 1
          ? `Move ${items[0].name} to a category`
          : `Move ${items.length} items to a category`
      }
      onClose={onClose}
    >
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <label className="flex flex-col gap-0.5">
          <span className="font-semibold text-qb-ink-muted">Category</span>
          <CategorySelect
            categories={categories}
            value={target}
            onChange={setTarget}
            autoFocus
          />
        </label>
        {error && (
          <Notice kind="error" text={error} onDismiss={() => setError(null)} />
        )}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button
            type="button"
            className="qb-btn qb-btn-secondary h-row-lg px-3"
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            type="submit"
            className="qb-btn qb-btn-primary h-row-lg px-3"
            disabled={busy}
          >
            Move
          </button>
        </div>
      </form>
    </Dialog>
  );
}

/**
 * A category filter for item reports and the item box: `control` is the select (nothing when
 * categories are off or none exist), `keeps(itemOrName)` whether an item stays in.
 */
export function useCategoryFilter(products, wanted = true) {
  const { enabled, categories, index } = useItemCategories();
  const [chosen, setChosen] = useState(undefined);
  const on = wanted && enabled && categories.length > 0;
  const byName = useMemo(
    () => new Map((products ?? []).map((p) => [p.name, p])),
    [products],
  );
  const keeps = (item) => {
    if (!on || chosen === undefined) return true;
    const p = typeof item === "string" ? byName.get(item) : item;
    return p ? index.covers(chosen, p.categoryId ?? null) : false;
  };
  const control = on ? (
    <CategorySelect
      categories={categories}
      value={chosen}
      onChange={setChosen}
      allLabel="All categories"
      className="qb-input h-row w-[170px] text-sm"
      aria-label="Item category"
    />
  ) : null;
  return { control, keeps, active: on && chosen !== undefined };
}
