import { useEffect, useRef, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import ClassSelect, { classLabel, showsClassColumn, useClassTracking } from './ClassSelect.jsx';
import { usePriceLevels } from './PriceLevels.jsx';
import NewItemDialog from './NewItemDialog.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatMoney, formatQuantity, parseMoneyToCents, parseQuantityE4, quantityAmount } from '../lib/money.js';
import { documentOrder, layOut, parsePercentPpm, percentText } from '../lib/itemTypes.js';

let nextKey = 1;
/** The item column's "Add new item…" entry. */
const NEW_ITEM = '__new';
/** The item column's fixed Subtotal entry: a subtotal is a line kind, not an item to set up. */
const SUBTOTAL = '__subtotal';
/** Item column values for groups: "group:<id>". */
const GROUP = 'group:';

/**
 * `kind` is ITEM (a product, service or fixed charge), SUBTOTAL, DISCOUNT, CHARGE (a percent
 * charge) or GROUP (a group's heading; its items follow it with `inGroup` set to its key).
 */
export const blankLine = (accountId) => ({ key: nextKey++, kind: 'ITEM', productId: null, description: '', accountId, classId: null, isTaxable: false, qty: '', rate: '' });

/** True for a line that is not a plain item: a subtotal, discount, percent charge or group heading. */
export const isFormLine = (line) => (line.kind ?? 'ITEM') !== 'ITEM';

/**
 * A stored document's item lines and form lines (subtotals) back as grid lines, in line order,
 * with a blank line to type into at the end.
 */
export function linesFromDocument(items, formLines, defaultAccountId) {
  // The heading whose items are being read, and how many are still to come.
  let open = null;
  const lines = documentOrder(items, formLines).map(({ item, form }) => {
    const line = toLine(item, form, defaultAccountId);
    if (line.kind === 'GROUP') open = { key: line.key, left: form.groupLineCount ?? 0 };
    else if (open && item && !item.percentPpm && open.left > 0) {
      line.inGroup = open.key;
      open.left -= 1;
      if (open.left === 0) open = null;
    }
    return line;
  });
  return [...lines, blankLine(defaultAccountId)];
}

function toLine(item, form, defaultAccountId) {
  return (
      item?.percentPpm
        ? {
            ...blankLine(item.incomeAccountId),
            kind: 'CHARGE',
            productId: item.productServiceId,
            classId: item.classId ?? null,
            isTaxable: Boolean(item.isTaxable),
            description: item.description,
            rate: percentText(item.percentPpm),
          }
        : form
        ? {
            ...blankLine(form.accountId ?? null),
            kind: form.kind,
            description: form.description,
            productId: form.productServiceId ?? null,
            classId: form.classId ?? null,
            isTaxable: Boolean(form.isTaxable),
            rate: form.kind !== 'DISCOUNT' ? '' : form.percentPpm ? percentText(form.percentPpm) : centsToInput(-form.amountCents),
            groupId: form.itemGroupId ?? null,
            printItems: form.printItems ?? true,
          }
        : {
            ...blankLine(item.incomeAccountId),
            productId: item.productServiceId,
            classId: item.classId ?? null,
            isTaxable: Boolean(item.isTaxable),
            description: item.description,
            qty: formatQuantity(item.quantityE4).replace(/,/g, ''),
            rate: centsToInput(item.rateCents),
          }
  );
}

/** Derives per-line state; blank lines are ignored, partially filled ones are invalid. */
export function evaluateLine(line) {
  if (line.kind === 'SUBTOTAL' || line.kind === 'GROUP') return { blank: false, form: true, amount: null, errors: {} };
  if (line.kind === 'CHARGE') {
    const percentPpm = parsePercentPpm(line.rate);
    return { blank: false, form: true, amount: null, percentPpm, errors: { account: line.accountId === null, rate: percentPpm === null } };
  }
  if (line.kind === 'DISCOUNT') {
    // An amount ("5.00") or a percentage of the line above ("10%").
    const text = line.rate.trim();
    const percentPpm = text.endsWith('%') ? parsePercentPpm(text) : null;
    const amountCents = text.endsWith('%') ? null : parseMoneyToCents(text);
    const badRate = text.endsWith('%') ? percentPpm === null : amountCents === null || amountCents <= 0;
    return { blank: false, form: true, amount: null, percentPpm, amountCents, errors: { account: line.accountId === null, rate: badRate } };
  }
  const blank = line.productId === null && line.description.trim() === '' && line.qty.trim() === '' && line.rate.trim() === '';
  if (blank) return { blank: true, amount: 0 };
  // Quantities may be fractional (up to 4 decimals) and are carried as integers scaled by 10,000.
  const qty = parseQuantityE4(line.qty);
  const rate = parseMoneyToCents(line.rate);
  const amount = qty !== null && rate !== null && rate > 0 ? quantityAmount(qty, rate) : null;
  return {
    blank: false,
    qty,
    rate,
    amount,
    errors: { account: line.accountId === null, qty: qty === null, rate: rate === null || rate <= 0 },
  };
}

/**
 * What a form's lines amount to, as text: the item lines and the subtotals, discounts, charges
 * and groups among them. Two forms with the same key would post the same lines (unsaved-changes
 * checks compare it).
 */
export function linesKey(summary) {
  return JSON.stringify({ lines: summary.payload, formLines: summary.formLines });
}

/** How many filled item lines follow the group heading at `index` and belong to it. */
function groupItems(lines, evaluated, index) {
  let count = 0;
  for (let j = index + 1; j < lines.length && lines[j].inGroup === lines[index].key && !isFormLine(lines[j]); j += 1) {
    if (!evaluated[j].blank) count += 1;
  }
  return count;
}

/**
 * Totals and validity across a set of lines. `payload` is the item lines, `formLines` the
 * subtotals placed among them, and `amountOf[i]` each grid line's amount (a subtotal's sum too).
 * `total` is the lines' total, net of line discounts: what the document discount comes off.
 */
export function summarizeLines(lines) {
  const evaluated = lines.map(evaluateLine);
  const filled = evaluated.filter((s) => !s.blank && !s.form);
  const invalidCount =
    filled.filter((s) => s.amount === null || Object.values(s.errors).some(Boolean)).length +
    evaluated.filter((s) => s.form && Object.values(s.errors).some(Boolean)).length;
  const itemsTotal = filled.reduce((sum, s) => (s.amount !== null ? sum + s.amount : sum), 0);
  const formLines = [];
  const formIndex = [];
  let itemsBefore = 0;
  lines.forEach((line, i) => {
    if (evaluated[i].form) {
      const st = evaluated[i];
      formLines.push(
        line.kind === 'DISCOUNT'
          ? {
              afterLines: itemsBefore,
              kind: 'DISCOUNT',
              description: line.description.trim(),
              productServiceId: line.productId,
              percentPpm: st.percentPpm,
              amountCents: st.amountCents,
              accountId: line.accountId,
              classId: line.classId ?? null,
              isTaxable: Boolean(line.isTaxable),
            }
          : line.kind === 'CHARGE'
            ? {
                afterLines: itemsBefore,
                kind: 'CHARGE',
                description: line.description.trim(),
                productServiceId: line.productId,
                percentPpm: st.percentPpm,
                accountId: line.accountId,
                classId: line.classId ?? null,
                isTaxable: Boolean(line.isTaxable),
              }
            : line.kind === 'GROUP'
              ? {
                  afterLines: itemsBefore,
                  kind: 'GROUP',
                  description: line.description.trim(),
                  itemGroupId: line.groupId ?? null,
                  // Its items: the filled item lines right after it that came from it.
                  groupLineCount: groupItems(lines, evaluated, i),
                  groupQuantityE4: 10_000,
                  printItems: line.printItems !== false,
                }
              : { afterLines: itemsBefore, kind: line.kind, description: line.description.trim() },
      );
      formIndex.push(i);
    } else if (!evaluated[i].blank) itemsBefore += 1;
  });
  // A heading whose items were all removed is dropped.
  for (let k = formLines.length - 1; k >= 0; k -= 1) {
    if (formLines[k].kind === 'GROUP' && formLines[k].groupLineCount === 0) {
      formLines.splice(k, 1);
      formIndex.splice(k, 1);
    }
  }
  const payload = lines
    .map((line, i) => ({ line, state: evaluated[i] }))
    .filter(({ state }) => !state.blank && !state.form)
    .map(({ line, state }) => ({
      productServiceId: line.productId,
      description: line.description.trim(),
      quantityE4: state.qty,
      rateCents: state.rate,
      incomeAccountId: line.accountId,
      classId: line.classId ?? null,
      isTaxable: Boolean(line.isTaxable),
      amountCents: state.amount,
    }));
  const layout = layOut(payload, formLines);
  const amountOf = evaluated.map((s) => s.amount);
  for (const row of layout.rows ?? []) if (row.form !== undefined) amountOf[formIndex[row.form]] = row.amountCents;
  // Trailing subtotals with nothing after them are fine; a form with only subtotals has no items.
  const total = layout.error ? itemsTotal : layout.linesCents;
  return { evaluated, filled, invalidCount, total, payload, formLines, amountOf, layoutError: layout.error ?? null };
}

const incomeOnly = (a) => a.classification === 'INCOME';

/**
 * Inline-editable line items. Clicking any cell turns that row into inputs immediately, focused on
 * the clicked column; Enter moves to the next row (adding one at the end), Escape leaves edit mode.
 *
 * With `customer` (the name typed on the form) and `customers`, a picked product's rate comes from
 * the customer's price level, and each line's Price level picker can change it. A line follows the
 * customer's level (`levelAuto`) until its level is picked by hand; a typed rate (`rateTyped`) is
 * never replaced.
 */
export default function LineItemGrid({ lines, setLines, accounts, products, defaultAccountId, currency = null, withTax = false, customer = '', customers = null, itemTypes = false }) {
  const money = (cents) => formatMoney(cents, currency);
  const { amountOf } = summarizeLines(lines);
  const groups = useLedgerQuery(api.listItemGroups).data ?? [];
  const [active, setActive] = useState(null);
  const focusCol = useRef('description');
  const tableRef = useRef(null);
  const withProducts = Array.isArray(products);
  const tracking = useClassTracking();
  const { classes } = tracking;
  const withClasses = showsClassColumn(tracking, lines);
  const pricing = usePriceLevels();
  const withLevels = withProducts && (pricing.levels.length > 0 || lines.some((l) => l.levelId != null));
  const customerRow = (customers ?? []).find((c) => c.name.toLowerCase() === String(customer ?? '').trim().toLowerCase());
  const customerLevel = pricing.levelOf(customerRow);
  /** The rate text for `productId` at `levelId` (null = the standard price), or null to keep the line's. */
  const rateAt = (productId, levelId) => {
    const product = products?.find((p) => p.id === productId);
    if (!product) return null;
    const cents = pricing.priceOf(levelId, productId) ?? product.rateCents;
    return cents > 0 ? centsToInput(cents) : null;
  };

  // A different customer (or a changed level): lines still following the customer's level re-price,
  // unless their rate was typed.
  useEffect(() => {
    setLines((ls) => {
      let changed = false;
      const next = ls.map((l) => {
        if (!l.levelAuto || l.rateTyped || l.productId == null) return l;
        const rate = rateAt(l.productId, customerLevel) ?? l.rate;
        if (l.levelId === customerLevel && l.rate === rate) return l;
        changed = true;
        return { ...l, levelId: customerLevel, rate };
      });
      return changed ? next : ls;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [customerLevel, pricing]);

  useEffect(() => {
    if (active === null) return;
    const el = tableRef.current?.querySelector(`[data-row="${active}"] [data-col="${focusCol.current}"]`);
    if (el && document.activeElement !== el) el.focus();
  }, [active]);

  const update = (index, patch) => setLines((ls) => ls.map((l, i) => (i === index ? { ...l, ...patch } : l)));
  const remove = (index) => {
    // Removing a group's heading leaves its items as ordinary lines.
    const key = lines[index]?.kind === 'GROUP' ? lines[index].key : null;
    const freed = (ls) => (key === null ? ls : ls.map((l) => (l.inGroup === key ? { ...l, inGroup: null } : l)));
    setLines((ls) => (ls.length > 1 ? freed(ls.filter((_, i) => i !== index)) : [blankLine(defaultAccountId)]));
    setActive(null);
  };
  const activate = (index, col) => {
    focusCol.current = col;
    setActive(index);
  };
  const onKeyDown = (e, index) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (index === lines.length - 1) setLines((ls) => [...ls, blankLine(defaultAccountId)]);
      activate(index + 1, withProducts ? 'product' : 'description');
    } else if (e.key === 'Escape') {
      setActive(null);
    }
  };
  // The line whose item column chose "＋ Add new item…", while its dialog is open.
  const [adding, setAdding] = useState(null);
  const pickProduct = (index, productId, known = null) => {
    if (productId === NEW_ITEM) return setAdding(index);
    const line = lines[index];
    if (typeof productId === 'string' && productId.startsWith(GROUP)) return expandGroup(index, Number(productId.slice(GROUP.length)));
    if (productId === SUBTOTAL) return update(index, { kind: 'SUBTOTAL', productId: null, description: 'Subtotal', qty: '', rate: '', isTaxable: false });
    const product = known ?? products.find((p) => p.id === productId);
    if (!product) return update(index, { kind: 'ITEM', productId: null, ...(isFormLine(line) ? { description: '', rate: '' } : {}) });
    if (product.itemType === 'DISCOUNT' || (product.itemType === 'OTHER_CHARGE' && product.percentPpm)) {
      return update(index, {
        kind: product.itemType === 'DISCOUNT' ? 'DISCOUNT' : 'CHARGE',
        productId,
        description: product.description ?? product.name,
        accountId: product.incomeAccountId,
        isTaxable: Boolean(product.isTaxable),
        qty: '',
        rate: product.percentPpm ? percentText(product.percentPpm) : product.rateCents > 0 ? centsToInput(product.rateCents) : '',
        levelId: null,
        levelAuto: false,
      });
    }
    // A new item's own rate: it isn't in the price lists yet.
    const rate = known ? (product.rateCents > 0 ? centsToInput(product.rateCents) : null) : rateAt(productId, customerLevel);
    update(index, {
      kind: 'ITEM',
      productId,
      description: product.description ?? product.name,
      accountId: product.incomeAccountId,
      isTaxable: Boolean(product.isTaxable),
      rate: rate ?? line.rate,
      qty: line.qty.trim() === '' || isFormLine(line) ? '1' : line.qty,
      levelId: customerLevel,
      levelAuto: true,
      rateTyped: false,
    });
  };
  // A group becomes its heading and one line per item, priced for the customer.
  const expandGroup = (index, groupId) => {
    const group = groups.find((g) => g.id === groupId);
    if (!group) return;
    const heading = { ...blankLine(lines[index].accountId), kind: 'GROUP', groupId, description: group.description ?? group.name, printItems: group.printItems };
    const members = group.members.map((m) => {
      const product = products.find((p) => p.id === m.productId);
      return {
        ...blankLine(product?.incomeAccountId ?? defaultAccountId),
        productId: m.productId,
        description: product?.description ?? product?.name ?? m.productName,
        isTaxable: Boolean(product?.isTaxable),
        qty: formatQuantity(m.quantityE4).replace(/,/g, ''),
        rate: rateAt(m.productId, customerLevel) ?? '',
        levelId: customerLevel,
        levelAuto: true,
        rateTyped: false,
        inGroup: heading.key,
      };
    });
    setLines((ls) => [...ls.slice(0, index), heading, ...members, ...ls.slice(index + 1)]);
    setActive(null);
  };
  const pickLevel = (index, levelId) => {
    const line = lines[index];
    update(index, { levelId, levelAuto: false, rateTyped: false, rate: (line.productId != null && rateAt(line.productId, levelId)) || line.rate });
  };
  // Columns a subtotal leaves empty: account, class, quantity, price level and rate.
  const spareColumns = 3 + (withClasses ? 1 : 0) + (withLevels ? 1 : 0);
  const levelName = (id) => pricing.levels.find((l) => l.id === id)?.name ?? '';

  return (
    <table ref={tableRef} className="qb-grid">
      <colgroup>
        <col className="w-[28px]" />
        {withProducts && <col className="w-[180px]" />}
        <col />
        <col className="w-[220px]" />
        {withClasses && <col className="w-[150px]" />}
        <col className="w-[64px]" />
        {withLevels && <col className="w-[120px]" />}
        <col className="w-[100px]" />
        <col className="w-[108px]" />
        {withTax && <col className="w-[40px]" />}
        <col className="w-[28px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head text-right">#</th>
          {withProducts && <th className="qb-grid-head">Product/Service</th>}
          <th className="qb-grid-head">Description</th>
          <th className="qb-grid-head">Income account</th>
          {withClasses && <th className="qb-grid-head">Class</th>}
          <th className="qb-grid-head text-right">Qty</th>
          {withLevels && <th className="qb-grid-head" title="Fills in the rate; a typed rate always wins">Price level</th>}
          <th className="qb-grid-head text-right">Rate</th>
          <th className="qb-grid-head text-right">Amount</th>
          {withTax && <th className="qb-grid-head text-center" title="Taxable">Tax</th>}
          <th className="qb-grid-head" aria-label="Remove" />
        </tr>
      </thead>
      <tbody>
        {lines.map((line, i) => {
          const state = evaluateLine(line);
          const isActive = active === i;
          const bad = (field) => !state.blank && state.errors[field];
          const cellProps = (col) => ({
            'data-cell': col,
            onMouseDown: (e) => {
              if (isActive) return;
              // The clicked text cell is swapped for an input during this event; without this the
              // browser's default mousedown focus lands on the removed node and blurs the new input.
              e.preventDefault();
              activate(i, col);
            },
          });
          const product = withProducts ? products.find((p) => p.id === line.productId) : null;
          const formLine = isFormLine(line);
          const amount = amountOf[i];
          return (
            <tr key={line.key} data-row={i} className={`qb-grid-row ${isActive ? 'is-active' : ''} ${formLine ? 'font-semibold' : ''} ${line.inGroup ? 'bg-qb-canvas' : ''}`}>
              <td className="qb-grid-cell text-right text-qb-ink-faint">{i + 1}</td>
              {isActive ? (
                <>
                  {withProducts && (
                    <td className="qb-grid-cell-edit">
                      <select
                        data-col="product"
                        value={line.kind === 'SUBTOTAL' ? SUBTOTAL : line.kind === 'GROUP' ? `${GROUP}${line.groupId}` : (line.productId ?? '')}
                        onChange={(e) => {
                          const v = e.target.value;
                          pickProduct(i, v === '' ? null : [NEW_ITEM, SUBTOTAL].includes(v) || v.startsWith(GROUP) ? v : Number(v));
                        }}
                        onKeyDown={(e) => onKeyDown(e, i)}
                      >
                        <option value="">—</option>
                        {products.filter((p) => p.isActive && (itemTypes || !(p.itemType === 'DISCOUNT' || p.percentPpm))).map((p) => (
                          <option key={p.id} value={p.id}>
                            {p.qtyOnHandE4 != null ? `${p.name} · ${formatQuantity(p.qtyOnHandE4)} on hand` : p.name}
                          </option>
                        ))}
                        {itemTypes &&
                          groups.filter((g) => g.isActive || g.id === line.groupId).map((g) => (
                            <option key={`g${g.id}`} value={`${GROUP}${g.id}`}>
                              Group: {g.name}
                            </option>
                          ))}
                        {itemTypes && <option value={SUBTOTAL}>Subtotal</option>}
                        <option value={NEW_ITEM}>＋ Add new item…</option>
                      </select>
                    </td>
                  )}
                  <td className="qb-grid-cell-edit">
                    <input
                      data-col="description"
                      value={line.description}
                      onChange={(e) => update(i, { description: e.target.value })}
                      onKeyDown={(e) => onKeyDown(e, i)}
                      placeholder="Description"
                    />
                  </td>
                  {line.kind === 'SUBTOTAL' || line.kind === 'GROUP' ? (
                    <td colSpan={spareColumns} className="qb-grid-cell">
                      {line.kind === 'GROUP' && (
                        <label className="flex items-center gap-1 font-normal" title="On the printed form, show the group as one line with its total">
                          <input type="checkbox" className="accent-qb-green" checked={line.printItems === false} onChange={(e) => update(i, { printItems: !e.target.checked })} />
                          Print as one line
                        </label>
                      )}
                    </td>
                  ) : (
                    <>
                      <td className="qb-grid-cell-edit">
                        <AccountSelect
                          data-col="account"
                          className=""
                          accounts={accounts}
                          filter={incomeOnly}
                          value={line.accountId}
                          onChange={(accountId) => update(i, { accountId })}
                          onKeyDown={(e) => onKeyDown(e, i)}
                          placeholder="Select account"
                          aria-invalid={bad('account')}
                        />
                      </td>
                      {withClasses && (
                        <td className="qb-grid-cell-edit">
                          <ClassSelect
                            data-col="class"
                            className=""
                            classes={classes}
                            value={line.classId ?? null}
                            onChange={(classId) => update(i, { classId })}
                            onKeyDown={(e) => onKeyDown(e, i)}
                            aria-label={`Class ${i + 1}`}
                          />
                        </td>
                      )}
                      {formLine ? <td className="qb-grid-cell" /> : <td className="qb-grid-cell-edit">
                        <input
                          data-col="qty"
                          inputMode="decimal"
                          className="num"
                          value={line.qty}
                          onChange={(e) => update(i, { qty: e.target.value })}
                          onKeyDown={(e) => onKeyDown(e, i)}
                          aria-invalid={bad('qty')}
                        />
                      </td>}
                      {withLevels && (formLine ? <td className="qb-grid-cell" /> :
                        <td className="qb-grid-cell-edit">
                          <select
                            data-col="level"
                            value={line.levelId ?? ''}
                            onChange={(e) => pickLevel(i, e.target.value === '' ? null : Number(e.target.value))}
                            onKeyDown={(e) => onKeyDown(e, i)}
                            aria-label={`Price level ${i + 1}`}
                          >
                            <option value="">Standard</option>
                            {pricing.levels.map((l) => (
                              <option key={l.id} value={l.id}>{l.name}</option>
                            ))}
                          </select>
                        </td>
                      )}
                      <td className="qb-grid-cell-edit">
                        <input
                          data-col="rate"
                          inputMode="decimal"
                          className="num"
                          value={line.rate}
                          onChange={(e) => update(i, { rate: e.target.value, rateTyped: true })}
                          placeholder={line.kind === 'DISCOUNT' ? '0.00 or %' : line.kind === 'CHARGE' ? '%' : undefined}
                          onKeyDown={(e) => onKeyDown(e, i)}
                          aria-invalid={bad('rate')}
                        />
                      </td>
                    </>
                  )}
                </>
              ) : (
                <>
                  {withProducts && (
                    <td className="qb-grid-cell" {...cellProps('product')}>
                      {line.kind === 'SUBTOTAL' ? 'Subtotal' : line.kind === 'GROUP' ? `Group: ${groups.find((g) => g.id === line.groupId)?.name ?? ''}` : (product?.name ?? '')}
                    </td>
                  )}
                  <td className="qb-grid-cell" title={line.description} {...cellProps('description')}>
                    {line.description}
                  </td>
                  {line.kind === 'SUBTOTAL' || line.kind === 'GROUP' ? (
                    <td colSpan={spareColumns} className="qb-grid-cell">
                      {line.kind === 'GROUP' && (
                        <label className="flex items-center gap-1 font-normal" title="On the printed form, show the group as one line with its total">
                          <input type="checkbox" className="accent-qb-green" checked={line.printItems === false} onChange={(e) => update(i, { printItems: !e.target.checked })} />
                          Print as one line
                        </label>
                      )}
                    </td>
                  ) : (
                    <>
                      <td className={`qb-grid-cell ${bad('account') ? 'text-qb-negative' : ''}`} {...cellProps('account')}>
                        {line.accountId !== null ? accountLabel(accounts, line.accountId) : state.blank ? '' : 'Select account'}
                      </td>
                      {withClasses && (
                        <td className="qb-grid-cell" {...cellProps('class')}>
                          {line.classId != null ? classLabel(classes, line.classId) : ''}
                        </td>
                      )}
                      <td className={`qb-grid-cell num ${bad('qty') ? 'bg-[#fdf0ef] text-qb-negative' : ''}`} {...cellProps('qty')}>
                        {line.qty}
                      </td>
                      {withLevels && (
                        <td className="qb-grid-cell" {...cellProps('level')}>
                          {line.levelId != null ? levelName(line.levelId) : ''}
                        </td>
                      )}
                      <td className={`qb-grid-cell num ${bad('rate') ? 'bg-[#fdf0ef] text-qb-negative' : ''}`} {...cellProps('rate')}>
                        {state.rate !== null && state.rate !== undefined && !bad('rate') ? money(state.rate) : line.rate}
                      </td>
                    </>
                  )}
                </>
              )}
              <td className="qb-grid-cell num font-semibold">
                {formLine ? (amount === null || amount === undefined ? '—' : money(amount)) : state.blank ? '' : state.amount !== null ? money(state.amount) : '—'}
              </td>
              {withTax && (
                <td className="qb-grid-cell cell-flush text-center">
                  {!['SUBTOTAL', 'GROUP'].includes(line.kind) && <input type="checkbox" className="accent-qb-green" checked={Boolean(line.isTaxable)} onChange={(e) => update(i, { isTaxable: e.target.checked })} aria-label={`Taxable ${i + 1}`} />}
                </td>
              )}
              <td className="qb-grid-cell cell-flush text-center">
                <button
                  type="button"
                  onClick={() => remove(i)}
                  className="h-row w-full text-qb-ink-faint hover:text-qb-negative"
                  aria-label={`Remove line ${i + 1}`}
                  title="Delete line"
                >
                  ×
                </button>
              </td>
            </tr>
          );
        })}
      </tbody>
      {adding !== null && (
        // A portal: the dialog renders on <body>, not in the table.
        <NewItemDialog
          accounts={accounts}
          defaultAccountId={lines[adding]?.accountId ?? defaultAccountId}
          onClose={() => setAdding(null)}
          onCreated={(product) => {
            pickProduct(adding, product.id, product);
            setAdding(null);
          }}
        />
      )}
    </table>
  );
}
