/**
 * The Bill To and Ship To blocks of a sales form, with the customer's P.O. number and the FOB
 * point. `header` is { billTo, shipTo, poNumber, fob }; `onChange(key, value)` sets one of them.
 * The addresses are the document's own copy: editing them here never changes the customer.
 */
export function SalesAddresses({ header, onChange, billLabel = 'Bill To', showShipping = true }) {
  const area = (key, title, extra = null) => (
    <label className="flex w-[230px] flex-col">
      <span className="flex items-center justify-between">
        <span className="qb-label">{title}</span>
        {extra}
      </span>
      <textarea
        className="qb-input h-[62px] resize-none py-0.5 leading-tight"
        value={header[key]}
        maxLength={500}
        spellCheck={false}
        onChange={(e) => onChange(key, e.target.value)}
      />
    </label>
  );
  return (
    <div className="flex flex-wrap items-start gap-2 border-b border-qb-border px-2 py-1">
      {area('billTo', billLabel)}
      {showShipping &&
        area(
          'shipTo',
          'Ship To',
          <button
            type="button"
            className="text-2xs text-qb-link hover:underline disabled:opacity-40"
            disabled={!header.billTo.trim()}
            title="Use the Bill To address for Ship To on this form"
            onClick={() => onChange('shipTo', header.billTo)}
          >
            Copy Bill To
          </button>,
        )}
      <label>
        <span className="qb-label">P.O. No.</span>
        <input className="qb-input w-[110px]" value={header.poNumber} maxLength={25} onChange={(e) => onChange('poNumber', e.target.value)} />
      </label>
      {showShipping && (
        <label>
          <span className="qb-label">FOB</span>
          <input className="qb-input w-[110px]" value={header.fob} maxLength={25} title="Free on board: where ownership passes to the customer" onChange={(e) => onChange('fob', e.target.value)} />
        </label>
      )}
    </div>
  );
}
