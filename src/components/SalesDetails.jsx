import { useEffect, useMemo, useRef, useState } from 'react';
import { SalesAddresses } from './SalesAddresses.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { ProfileEntrySelect } from './ProfileFields.jsx';
import DateBox from './DateBox.jsx';
import { useSalesPrefs } from '../lib/salesPrefs.js';

/**
 * Rep, ship date, Ship Via, the customer custom fields marked for sales forms and the Customer
 * Message, for the header of an invoice, sales receipt or estimate. The rep, ship and field boxes
 * show only once their list has an entry (or the document being loaded already has a value), so a
 * company that does not use them sees nothing; the Customer Message box is always there, with
 * "Add new" to start the list. The rep and fields fill in from the customer until the user changes
 * them. A new form starts with the company's usual Ship Via (Preferences › Sales & Customers).
 *
 * `addresses` is the Bill To / Ship To / P.O. No. / FOB row. The addresses fill in from the customer
 * (a job uses its parent's when it has none) until the user edits them; the document keeps what
 * was on the form when it was saved.
 */
export function useSalesDetails(customerName, customers, { billLabel = 'Bill To' } = {}) {
  const reps = useLedgerQuery(api.listSalesReps).data ?? [];
  const methods = useLedgerQuery(api.listShipMethods).data ?? [];
  const allFields = useLedgerQuery(api.listCustomFields).data;
  const formFields = useMemo(() => (allFields ?? []).filter((f) => f.list === 'CUSTOMER' && f.onSalesForms), [allFields]);
  const [repId, setRepId] = useState(null);
  const [repTouched, setRepTouched] = useState(false);
  const [shipDate, setShipDate] = useState('');
  const [shipMethodId, setShipMethodId] = useState(null);
  const [messageId, setMessageId] = useState(null);
  // The form style (template) this document prints with; null: the customer's style, or the default.
  const styles = useLedgerQuery(api.listFormStyles).data ?? [];
  const [styleId, setStyleId] = useState(null);
  // The company defaults go on a new form once preferences load, unless a document was loaded.
  const { defaultShipMethodId, defaultFob, loaded: prefsLoaded } = useSalesPrefs();
  const [defaulted, setDefaulted] = useState(false);
  useEffect(() => {
    if (!prefsLoaded || defaulted) return;
    setDefaulted(true);
    setShipMethodId(defaultShipMethodId);
    // The usual F.O.B., unless one was typed or a document was loaded meanwhile.
    setHeader((h) => (h.fob ? h : { ...h, fob: defaultFob }));
  }, [prefsLoaded, defaulted, defaultShipMethodId, defaultFob]);
  const [values, setValues] = useState({}); // field id → value
  const [touched, setTouched] = useState(() => new Set()); // field ids the user typed in
  const [header, setHeader] = useState({ billTo: '', shipTo: '', poNumber: '', fob: '' });
  // Which address boxes the user (or a loaded document) set, so a customer change leaves them. A
  // ref, so defaults that arrive after the user has started typing see the latest value.
  const addressTouched = useRef({ billTo: false, shipTo: false });
  const [refill, setRefill] = useState(0);
  const customer = (customers ?? []).find((c) => c.name.toLowerCase() === (customerName ?? '').trim().toLowerCase());

  useEffect(() => {
    if (!repTouched) setRepId(customer?.salesRepId ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [customer?.id, customer?.salesRepId]);

  useEffect(() => {
    if (addressTouched.current.billTo && addressTouched.current.shipTo) return undefined;
    let live = true;
    const fill = (defaults) =>
      live &&
      setHeader((h) => ({
        ...h,
        billTo: addressTouched.current.billTo ? h.billTo : (defaults?.billTo ?? ''),
        shipTo: addressTouched.current.shipTo ? h.shipTo : (defaults?.shipTo ?? ''),
      }));
    if (customer) api.customerSalesDefaults(customer.id).then(fill, () => {});
    else fill(null);
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [customer?.id, refill]);

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

  const messageBox = (
    <label>
      <span className="qb-label">Customer message</span>
      <ProfileEntrySelect list="CUSTOMER_MESSAGE" noun="customer message" value={messageId} onChange={setMessageId} className="qb-input w-[180px]" ariaLabel="Customer message" />
    </label>
  );

  const picker = (
      <>
        <label title="The form style this document prints, saves as PDF and emails with (Settings › Custom Forms)">
          <span className="qb-label">Template</span>
          <select className="qb-input w-[150px]" value={styleId ?? ''} onChange={(e) => setStyleId(e.target.value === '' ? null : Number(e.target.value))} aria-label="Template">
            <option value="">{customer?.formStyleId ? 'Customer’s style' : 'Default style'}</option>
            {styles.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </label>
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
              <DateBox className="qb-input w-[132px]" value={shipDate} onChange={(e) => setShipDate(e.target.value)} />
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
        {messageBox}
      </>
  );

  const addresses = (
    <SalesAddresses
      header={header}
      billLabel={billLabel}
      onChange={(key, value) => {
        if (key === 'billTo' || key === 'shipTo') addressTouched.current = { ...addressTouched.current, [key]: true };
        setHeader((h) => ({ ...h, [key]: value }));
      }}
    />
  );

  return {
    picker,
    addresses,
    /** The details for a form's input; spread into it. */
    payload: () => ({
      salesRepId: repId,
      shipDate: shipDate || null,
      shipMethodId,
      customerMessageId: messageId,
      formStyleId: styleId,
      customFields: formFields.map((f) => ({ fieldId: f.id, value: (values[f.id] ?? '').trim() })).filter((f) => f.value !== ''),
      billTo: header.billTo,
      shipTo: header.shipTo,
      poNumber: header.poNumber.trim() || null,
      fob: header.fob.trim() || null,
    }),
    /** Loads a saved document's details (an edit, or an estimate becoming an invoice). */
    set: (details) => {
      setRepTouched(true);
      setRepId(details?.salesRepId ?? null);
      setShipDate(details?.shipDate ?? '');
      setDefaulted(true);
      setShipMethodId(details?.shipMethodId ?? null);
      setMessageId(details?.customerMessageId ?? null);
      setStyleId(details?.formStyleId ?? null);
      const loaded = Object.fromEntries((details?.customFields ?? []).map((f) => [f.fieldId, f.value]));
      setValues(loaded);
      setTouched(new Set(formFields.map((f) => f.id)));
      // A document written before forms kept addresses has none: fill from the customer.
      const has = { billTo: details?.billTo != null, shipTo: details?.shipTo != null };
      addressTouched.current = has;
      setHeader({ billTo: details?.billTo ?? '', shipTo: details?.shipTo ?? '', poNumber: details?.poNumber ?? '', fob: details?.fob ?? '' });
      if (!has.billTo || !has.shipTo) setRefill((n) => n + 1);
    },
    reset: () => {
      setRepTouched(false);
      setRepId(null);
      setShipDate('');
      // A new form takes the company's Ship Via, now or once preferences load.
      setDefaulted(prefsLoaded);
      setShipMethodId(defaultShipMethodId);
      setMessageId(null);
      setStyleId(null);
      setValues({});
      setTouched(new Set());
      addressTouched.current = { billTo: false, shipTo: false };
      setHeader({ billTo: '', shipTo: '', poNumber: '', fob: prefsLoaded ? defaultFob : '' });
      setRefill((n) => n + 1);
    },
  };
}
