import { useEffect, useRef, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import ClassSelect, { classLabel, showsClassColumn, useClassTracking } from './ClassSelect.jsx';
import { centsToInput, formatMoney, formatQuantity, parseMoneyToCents, parseQuantityE4, quantityAmount } from '../lib/money.js';

let nextKey = 1;
export const blankLine = (accountId) => ({ key: nextKey++, productId: null, description: '', accountId, classId: null, isTaxable: false, qty: '', rate: '' });

/** Derives per-line state; blank lines are ignored, partially filled ones are invalid. */
export function evaluateLine(line) {
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

/** Totals and validity across a set of lines. */
export function summarizeLines(lines) {
  const evaluated = lines.map(evaluateLine);
  const filled = evaluated.filter((s) => !s.blank);
  const invalidCount = filled.filter((s) => s.amount === null || Object.values(s.errors).some(Boolean)).length;
  const total = filled.reduce((sum, s) => (s.amount !== null ? sum + s.amount : sum), 0);
  const payload = lines
    .map((line, i) => ({ line, state: evaluated[i] }))
    .filter(({ state }) => !state.blank)
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
  return { evaluated, filled, invalidCount, total, payload };
}

const incomeOnly = (a) => a.classification === 'INCOME';

/**
 * Inline-editable line items. Clicking any cell turns that row into inputs immediately, focused on
 * the clicked column; Enter moves to the next row (adding one at the end), Escape leaves edit mode.
 */
export default function LineItemGrid({ lines, setLines, accounts, products, defaultAccountId, currency = null, withTax = false }) {
  const money = (cents) => formatMoney(cents, currency);
  const [active, setActive] = useState(null);
  const focusCol = useRef('description');
  const tableRef = useRef(null);
  const withProducts = Array.isArray(products);
  const tracking = useClassTracking();
  const { classes } = tracking;
  const withClasses = showsClassColumn(tracking, lines);

  useEffect(() => {
    if (active === null) return;
    const el = tableRef.current?.querySelector(`[data-row="${active}"] [data-col="${focusCol.current}"]`);
    if (el && document.activeElement !== el) el.focus();
  }, [active]);

  const update = (index, patch) => setLines((ls) => ls.map((l, i) => (i === index ? { ...l, ...patch } : l)));
  const remove = (index) => {
    setLines((ls) => (ls.length > 1 ? ls.filter((_, i) => i !== index) : [blankLine(defaultAccountId)]));
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
  const pickProduct = (index, productId) => {
    const product = products.find((p) => p.id === productId);
    if (!product) return update(index, { productId: null });
    const line = lines[index];
    update(index, {
      productId,
      description: product.description ?? product.name,
      accountId: product.incomeAccountId,
      isTaxable: Boolean(product.isTaxable),
      rate: product.rateCents > 0 ? centsToInput(product.rateCents) : line.rate,
      qty: line.qty.trim() === '' ? '1' : line.qty,
    });
  };

  return (
    <table ref={tableRef} className="qb-grid">
      <colgroup>
        <col className="w-[28px]" />
        {withProducts && <col className="w-[180px]" />}
        <col />
        <col className="w-[220px]" />
        {withClasses && <col className="w-[150px]" />}
        <col className="w-[64px]" />
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
            onMouseDown: (e) => {
              if (isActive) return;
              // The clicked text cell is swapped for an input during this event; without this the
              // browser's default mousedown focus lands on the removed node and blurs the new input.
              e.preventDefault();
              activate(i, col);
            },
          });
          const product = withProducts ? products.find((p) => p.id === line.productId) : null;
          return (
            <tr key={line.key} data-row={i} className={`qb-grid-row ${isActive ? 'is-active' : ''}`}>
              <td className="qb-grid-cell text-right text-qb-ink-faint">{i + 1}</td>
              {isActive ? (
                <>
                  {withProducts && (
                    <td className="qb-grid-cell-edit">
                      <select
                        data-col="product"
                        value={line.productId ?? ''}
                        onChange={(e) => pickProduct(i, e.target.value === '' ? null : Number(e.target.value))}
                        onKeyDown={(e) => onKeyDown(e, i)}
                      >
                        <option value="">—</option>
                        {products.filter((p) => p.isActive).map((p) => (
                          <option key={p.id} value={p.id}>
                            {p.qtyOnHandE4 != null ? `${p.name} · ${formatQuantity(p.qtyOnHandE4)} on hand` : p.name}
                          </option>
                        ))}
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
                  <td className="qb-grid-cell-edit">
                    <input
                      data-col="qty"
                      inputMode="decimal"
                      className="num"
                      value={line.qty}
                      onChange={(e) => update(i, { qty: e.target.value })}
                      onKeyDown={(e) => onKeyDown(e, i)}
                      aria-invalid={bad('qty')}
                    />
                  </td>
                  <td className="qb-grid-cell-edit">
                    <input
                      data-col="rate"
                      inputMode="decimal"
                      className="num"
                      value={line.rate}
                      onChange={(e) => update(i, { rate: e.target.value })}
                      onKeyDown={(e) => onKeyDown(e, i)}
                      aria-invalid={bad('rate')}
                    />
                  </td>
                </>
              ) : (
                <>
                  {withProducts && (
                    <td className="qb-grid-cell" {...cellProps('product')}>
                      {product?.name ?? ''}
                    </td>
                  )}
                  <td className="qb-grid-cell" title={line.description} {...cellProps('description')}>
                    {line.description}
                  </td>
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
                  <td className={`qb-grid-cell num ${bad('rate') ? 'bg-[#fdf0ef] text-qb-negative' : ''}`} {...cellProps('rate')}>
                    {state.rate !== null && state.rate !== undefined && !bad('rate') ? money(state.rate) : line.rate}
                  </td>
                </>
              )}
              <td className="qb-grid-cell num font-semibold">
                {state.blank ? '' : state.amount !== null ? money(state.amount) : '—'}
              </td>
              {withTax && (
                <td className="qb-grid-cell cell-flush text-center">
                  <input type="checkbox" className="accent-qb-green" checked={Boolean(line.isTaxable)} onChange={(e) => update(i, { isTaxable: e.target.checked })} aria-label={`Taxable ${i + 1}`} />
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
    </table>
  );
}
