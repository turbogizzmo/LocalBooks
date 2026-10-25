import { useEffect, useMemo, useState } from 'react';
import { api, useLedgerQuery } from '../lib/api.js';

/**
 * Rep, ship date, Ship Via and the customer custom fields marked for sales forms, for the header
 * of an invoice, sales receipt or estimate. Each box shows only once its list has an entry (or the
 * document being loaded already has a value), so a company that does not use them sees nothing.
 * The rep and fields fill in from the customer until the user changes them.
 */
export function useSalesDetails(customerName, customers) {
  const reps = useLedgerQuery(api.listSalesReps).data ?? [];
  const methods = useLedgerQuery(api.listShipMethods).data ?? [];
  const allFields = useLedgerQuery(api.listCustomFields).data;
  const formFields = useMemo(() => (allFields ?? []).filter((f) => f.list === 'CUSTOMER' && f.onSalesForms), [allFields]);
  const [repId, setRepId] = useState(null);
  const [repTouched, setRepTouched] = useState(false);
  const [shipDate, setShipDate] = useState('');
  const [shipMethodId, setShipMethodId] = useState(null);
  const [values, setValues] = useState({}); // field id → value
  const [touched, setTouched] = useState(() => new Set()); // field ids the user typed in
  const customer = (customers ?? []).find((c) => c.name.toLowerCase() === (customerName ?? '').trim().toLowerCase());

  useEffect(() => {
    if (!repTouched) setRepId(customer?.salesRepId ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [customer?.id, customer?.salesRepId]);

  useEffect(() => {
    if (formFields.length === 0) return undefined;
    let live = true;
    const fill = (fields) =>
      live &&
      setValues((v) => {
        const next = { ...v };
        for (const f of formFields) if (!touched.has(f.id)) next[f.id] = fields.find((x) => x.fieldId === f.id)?.value ?? '';
        return next;
      });
    if (customer) api.getCustomValues('CUSTOMER', customer.id).then(fill, () => {});
    else fill([]);
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [customer?.id, formFields.length]);

  const showRep = reps.some((r) => r.isActive) || repId != null;
  const showShip = methods.some((m) => m.isActive) || shipMethodId != null || shipDate !== '';
  const repOptions = reps.filter((r) => r.isActive || r.id === repId);
  const methodOptions = methods.filter((m) => m.isActive || m.id === shipMethodId);

  const picker =
    showRep || showShip || formFields.length > 0 ? (
      <>
        {showRep && (
          <label>
            <span className="qb-label">Rep</span>
            <select
              className="qb-input w-[76px]"
              value={repId ?? ''}
              title={reps.find((r) => r.id === repId)?.name ?? undefined}
              onChange={(e) => {
                setRepTouched(true);
                setRepId(e.target.value === '' ? null : Number(e.target.value));
              }}
            >
              <option value="" />
              {repOptions.map((r) => (
                <option key={r.id} value={r.id} title={r.name ?? undefined}>
                  {r.initials}
                </option>
              ))}
            </select>
          </label>
        )}
        {showShip && (
          <>
            <label>
              <span className="qb-label">Ship date</span>
              <input type="date" className="qb-input w-[132px]" value={shipDate} onChange={(e) => setShipDate(e.target.value)} />
            </label>
            <label>
              <span className="qb-label">Ship via</span>
              <select className="qb-input w-[120px]" value={shipMethodId ?? ''} onChange={(e) => setShipMethodId(e.target.value === '' ? null : Number(e.target.value))}>
                <option value="" />
                {methodOptions.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}
                  </option>
                ))}
              </select>
            </label>
          </>
        )}
        {formFields.map((f) => (
          <label key={f.id}>
            <span className="qb-label">{f.label}</span>
            <input
              className="qb-input w-[120px]"
              value={values[f.id] ?? ''}
              maxLength={100}
              onChange={(e) => {
                setTouched((t) => new Set(t).add(f.id));
                setValues((v) => ({ ...v, [f.id]: e.target.value }));
              }}
            />
          </label>
        ))}
      </>
    ) : null;

  return {
    picker,
    /** The details for a form's input; spread into it. */
    payload: () => ({
      salesRepId: repId,
      shipDate: shipDate || null,
      shipMethodId,
      customFields: formFields.map((f) => ({ fieldId: f.id, value: (values[f.id] ?? '').trim() })).filter((f) => f.value !== ''),
    }),
    /** Loads a saved document's details (an edit, or an estimate becoming an invoice). */
    set: (details) => {
      setRepTouched(true);
      setRepId(details?.salesRepId ?? null);
      setShipDate(details?.shipDate ?? '');
      setShipMethodId(details?.shipMethodId ?? null);
      const loaded = Object.fromEntries((details?.customFields ?? []).map((f) => [f.fieldId, f.value]));
      setValues(loaded);
      setTouched(new Set(formFields.map((f) => f.id)));
    },
    reset: () => {
      setRepTouched(false);
      setRepId(null);
      setShipDate('');
      setShipMethodId(null);
      setValues({});
      setTouched(new Set());
    },
  };
}
