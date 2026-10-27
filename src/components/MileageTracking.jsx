import { useEffect, useMemo, useRef, useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, formatQuantity, parseMoneyToCents, parseQuantityE4 } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';

/**
 * Mileage (`mileage.rs`): trips by vehicle, the vehicle list, standard mileage rates, and billing
 * billable trips on an invoice. Trips don't post to the books; a billed trip posts through its invoice.
 */

/** Miles or an odometer reading ("12.5"), scaled by 10,000; "0" is allowed for an odometer. */
const parseMiles = (text, allowZero = false) => (allowZero && /^\s*0*(\.0*)?\s*$/.test(text) && text.trim() !== '' ? 0 : parseQuantityE4(text));
const milesText = (e4) => (e4 === null || e4 === undefined ? '' : formatQuantity(e4));
const rateText = (e4) => `$${formatQuantity(e4)}/mi`;

const TABS = [
  ['trips', 'Trips'],
  ['bill', 'Bill trips'],
  ['vehicles', 'Vehicles'],
  ['rates', 'Mileage rates'],
];

function TripForm({ vehicles, customers, editing, onDone }) {
  const blank = { vehicleId: '', date: todayIso(), start: '', end: '', total: '', notes: '', customerId: '', billable: false };
  const [form, setForm] = useState(() =>
    editing
      ? {
          vehicleId: String(editing.vehicleId),
          date: editing.tripDate,
          start: milesText(editing.odometerStartE4),
          end: milesText(editing.odometerEndE4),
          total: editing.odometerStartE4 === null ? milesText(editing.milesE4) : '',
          notes: editing.notes ?? '',
          customerId: editing.customerId === null ? '' : String(editing.customerId),
          billable: editing.billable,
        }
      : blank,
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const vehicleRef = useRef(null);
  useEffect(() => vehicleRef.current?.focus(), []);
  const set = (key) => (e) => setForm((f) => ({ ...f, [key]: e.target.type === 'checkbox' ? e.target.checked : e.target.value }));

  const useOdometer = form.start.trim() !== '' || form.end.trim() !== '';
  const start = parseMiles(form.start, true);
  const end = parseMiles(form.end);
  const odometerMiles = start !== null && end !== null && end > start ? end - start : null;
  const miles = useOdometer ? odometerMiles : parseMiles(form.total);

  const save = async () => {
    if (form.vehicleId === '') return setError('Choose a vehicle.');
    if (useOdometer && odometerMiles === null) return setError('Enter both odometer readings, with the ending one higher.');
    if (!useOdometer && miles === null) return setError('Enter the total miles (like 12.5) or both odometer readings.');
    if (form.billable && form.customerId === '') return setError('A billable trip needs a customer or job.');
    setBusy(true);
    const input = {
      vehicleId: Number(form.vehicleId),
      tripDate: form.date,
      odometerStartE4: useOdometer ? start : null,
      odometerEndE4: useOdometer ? end : null,
      milesE4: useOdometer ? null : miles,
      notes: form.notes.trim() || null,
      customerId: form.customerId === '' ? null : Number(form.customerId),
      billable: form.billable,
    };
    try {
      const saved = editing ? await api.updateMileageTrip(editing.id, input) : await api.createMileageTrip(input);
      onDone(`${editing ? 'Updated' : 'Recorded'} ${milesText(saved.milesE4)} miles for ${saved.vehicleName} on ${formatDate(saved.tripDate)}.`);
      if (!editing) setForm({ ...blank, vehicleId: form.vehicleId, date: form.date });
      setError(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-wrap items-end gap-2 px-2 py-1">
      <label>
        <span className="qb-label">Vehicle</span>
        <select ref={vehicleRef} className="qb-input w-[170px]" value={form.vehicleId} onChange={set('vehicleId')}>
          <option value="">Choose a vehicle…</option>
          {vehicles.filter((v) => v.isActive || String(v.id) === form.vehicleId).map((v) => (
            <option key={v.id} value={v.id}>{v.name}</option>
          ))}
        </select>
      </label>
      <label>
        <span className="qb-label">Trip date</span>
        <input type="date" className="qb-input w-[132px]" value={form.date} onChange={set('date')} />
      </label>
      <label>
        <span className="qb-label">Odometer start</span>
        <input className="qb-input num w-[100px]" value={form.start} onChange={set('start')} inputMode="decimal" />
      </label>
      <label>
        <span className="qb-label">Odometer end</span>
        <input className="qb-input num w-[100px]" value={form.end} onChange={set('end')} inputMode="decimal" />
      </label>
      <label>
        <span className="qb-label">Total miles</span>
        <input
          className="qb-input num w-[90px]"
          value={useOdometer ? milesText(odometerMiles) : form.total}
          onChange={set('total')}
          disabled={useOdometer}
          inputMode="decimal"
          title={useOdometer ? 'Worked out from the odometer readings' : undefined}
        />
      </label>
      <label>
        <span className="qb-label">Customer:Job</span>
        <select className="qb-input w-[200px]" value={form.customerId} onChange={set('customerId')}>
          <option value="">—</option>
          {customers.filter((c) => c.isActive || String(c.id) === form.customerId).map((c) => (
            <option key={c.id} value={c.id}>{c.name}</option>
          ))}
        </select>
      </label>
      <label className="flex h-row items-center gap-1 text-sm">
        <input type="checkbox" checked={form.billable} onChange={set('billable')} />
        Billable
      </label>
      <label className="min-w-[180px] flex-1">
        <span className="qb-label">Notes / purpose</span>
        <input
          className="qb-input w-full"
          value={form.notes}
          onChange={set('notes')}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              save();
            }
          }}
        />
      </label>
      <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
        {editing ? 'Save changes' : 'Save trip'}
      </button>
      {editing && (
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => onDone(null)}>
          Cancel
        </button>
      )}
      {error && <div className="w-full text-sm text-qb-negative">{error}</div>}
    </div>
  );
}

function TripsTab({ vehicles, customers, intent, onNotice }) {
  const [range, setRange] = useState(() => [addDays(todayIso(), -30), todayIso()]);
  const [editing, setEditing] = useState(null);
  const [formKey, setFormKey] = useState(0);
  const { data: trips, error, reload } = useLedgerQuery(() => api.listMileageTrips(range[0], range[1]));
  useEffect(() => {
    reload();
  }, [range, reload]);
  useEffect(() => {
    setEditing(null);
    setFormKey((k) => k + 1);
  }, [intent]);
  const sorter = useSort(trips, {
    date: (t) => t.tripDate,
    vehicle: (t) => t.vehicleName,
    customer: (t) => t.customerName,
    notes: (t) => t.notes,
    billable: (t) => (t.invoiceNumber ? 2 : t.billable ? 1 : 0),
    miles: (t) => t.milesE4,
    deductible: (t) => t.deductibleCents,
  });
  const totals = useMemo(() => {
    const list = trips ?? [];
    return { miles: list.reduce((s, t) => s + t.milesE4, 0), deductible: list.reduce((s, t) => s + (t.deductibleCents ?? 0), 0), noRate: list.filter((t) => t.rateE4 === null).length };
  }, [trips]);

  const remove = async (trip) => {
    try {
      await api.deleteMileageTrip(trip.id);
      onNotice({ kind: 'success', text: `Deleted the ${formatDate(trip.tripDate)} trip.` });
    } catch (err) {
      onNotice({ kind: 'error', text: errorText(err) });
    }
  };

  return (
    <>
      <section className="qb-panel flex-none">
        <div className="flex items-center gap-2 border-b border-qb-border px-2 py-1">
          <h2 className="text-md font-semibold">{editing ? 'Change trip' : 'Enter vehicle mileage'}</h2>
          <span className="text-xs text-qb-ink-muted">Trips are records and don’t post to your books; a billed trip posts through its invoice</span>
        </div>
        {vehicles.length === 0 ? (
          <div className="px-2 py-1.5 text-sm text-qb-ink-muted">Add a vehicle on the Vehicles tab first.</div>
        ) : (
          <TripForm
            key={editing ? `edit-${editing.id}` : `new-${formKey}`}
            vehicles={vehicles}
            customers={customers}
            editing={editing}
            onDone={(text) => {
              setEditing(null);
              if (text) onNotice({ kind: 'success', text });
            }}
          />
        )}
      </section>
      {error && <Notice kind="error" text={`Could not load trips: ${error}`} />}
      {totals.noRate > 0 && <Notice kind="info" text={`${totals.noRate} trip${totals.noRate === 1 ? ' is' : 's are'} dated before your first mileage rate, so ${totals.noRate === 1 ? 'it has' : 'they have'} no deductible amount. Add the rate on the Mileage rates tab.`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Trips</h2>
          <input type="date" className="qb-input h-row w-[128px] text-sm" value={range[0]} onChange={(e) => setRange([e.target.value, range[1]])} aria-label="From" />
          <span className="text-sm text-qb-ink-muted">to</span>
          <input type="date" className="qb-input h-row w-[128px] text-sm" value={range[1]} onChange={(e) => setRange([range[0], e.target.value])} aria-label="To" />
          <span className="ml-auto text-sm text-qb-ink-muted">
            Total <span className="num font-semibold text-qb-ink">{milesText(totals.miles)} mi</span> · deductible{' '}
            <span className="num font-semibold text-qb-ink">{formatCents(totals.deductible)}</span>
          </span>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <thead>
              <tr>
                <SortTh sorter={sorter} col="date">Date</SortTh>
                <SortTh sorter={sorter} col="vehicle">Vehicle</SortTh>
                <SortTh sorter={sorter} col="customer">Customer:Job</SortTh>
                <SortTh sorter={sorter} col="notes">Notes</SortTh>
                <SortTh sorter={sorter} col="billable">Billable</SortTh>
                <SortTh sorter={sorter} col="miles" className="text-right">Miles</SortTh>
                <th className="qb-grid-head text-right">Rate</th>
                <SortTh sorter={sorter} col="deductible" className="text-right">Deductible</SortTh>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {trips && trips.length === 0 && (
                <tr>
                  <td colSpan={9} className="qb-grid-cell text-qb-ink-muted">No trips in this range.</td>
                </tr>
              )}
              {sorter.sorted.map((t) => (
                <tr key={t.id} className="qb-grid-row">
                  <td className="qb-grid-cell">{formatDate(t.tripDate)}</td>
                  <td className="qb-grid-cell font-semibold">{t.vehicleName}</td>
                  <td className="qb-grid-cell">{t.customerName ?? ''}</td>
                  <td className="qb-grid-cell text-qb-ink-muted" title={t.notes ?? ''}>{t.notes ?? ''}</td>
                  <td className="qb-grid-cell">
                    {t.invoiceNumber ? <span className="text-qb-ink-muted">Billed #{t.invoiceNumber}</span> : t.billable ? <span className="font-semibold text-qb-green-active">✓ Unbilled</span> : <span className="text-qb-ink-faint">No</span>}
                  </td>
                  <td className="qb-grid-cell num font-semibold" title={t.odometerStartE4 !== null ? `Odometer ${milesText(t.odometerStartE4)} – ${milesText(t.odometerEndE4)}` : undefined}>{milesText(t.milesE4)}</td>
                  <td className="qb-grid-cell num">{t.rateE4 !== null ? rateText(t.rateE4) : '—'}</td>
                  <td className="qb-grid-cell num">{t.deductibleCents !== null ? formatCents(t.deductibleCents) : '—'}</td>
                  <td className="qb-grid-cell whitespace-nowrap text-right">
                    {!t.invoiceNumber && (
                      <>
                        <button type="button" className="text-sm text-qb-link hover:underline" onClick={() => setEditing(t)}>Edit</button>
                        <button type="button" className="ml-2 text-sm text-qb-link hover:underline" onClick={() => remove(t)}>Delete</button>
                      </>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </>
  );
}

function BillTab({ products, onNotice, navigate }) {
  const { data: unbilled, error } = useLedgerQuery(() => api.listUnbilledMileage(null));
  const jobs = useMemo(() => {
    const map = new Map();
    for (const t of unbilled ?? []) map.set(t.customerId, t.customerName);
    return [...map.entries()];
  }, [unbilled]);
  const [chosenCustomer, setChosenCustomer] = useState('');
  // Every trip is billed unless unticked.
  const [excluded, setExcluded] = useState(new Set());
  const items = products.filter((p) => p.isActive);
  const [itemId, setItemId] = useState('');
  const [rate, setRate] = useState('');
  const [issue, setIssue] = useState(todayIso());
  const [due, setDue] = useState(addDays(todayIso(), 30));
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState(null);

  // The mileage item: one named like "Mileage" if there is one.
  useEffect(() => {
    if (itemId !== '' || items.length === 0) return;
    const guess = items.find((p) => /mile/i.test(p.name));
    if (guess) pickItem(String(guess.id));
  }, [items.length]); // eslint-disable-line react-hooks/exhaustive-deps

  const customerId = jobs.some(([id]) => String(id) === chosenCustomer) ? chosenCustomer : jobs.length > 0 ? String(jobs[0][0]) : '';
  const trips = (unbilled ?? []).filter((t) => String(t.customerId) === customerId);
  const chooseCustomer = (id) => {
    setChosenCustomer(id);
    setExcluded(new Set());
  };
  function pickItem(id) {
    setItemId(id);
    const item = products.find((p) => String(p.id) === id);
    if (item && item.rateCents > 0) setRate(centsToInput(item.rateCents));
  }
  const rateCents = parseMoneyToCents(rate);
  const chosen = trips.filter((t) => !excluded.has(t.id));
  const miles = chosen.reduce((s, t) => s + t.milesE4, 0);
  const estimate = rateCents ? chosen.reduce((s, t) => s + Math.round((t.milesE4 * rateCents) / 10_000), 0) : null;

  const bill = async () => {
    if (chosen.length === 0) return setFormError('Choose at least one trip.');
    if (itemId === '') return setFormError('Choose the item to bill mileage as (for example a service item named Mileage).');
    if (!rateCents || rateCents <= 0) return setFormError('Enter the sales rate per mile.');
    if (due < issue) return setFormError('The due date is before the invoice date.');
    setBusy(true);
    try {
      const invoice = await api.billMileageTrips({ customerId: Number(customerId), tripIds: chosen.map((t) => t.id), productServiceId: Number(itemId), rateCents, issueDate: issue, dueDate: due });
      setFormError(null);
      setExcluded(new Set());
      onNotice({ kind: 'success', text: `Invoice #${invoice.invoiceNumber} for ${invoice.customerName}: ${formatCents(invoice.totalCents)} for ${milesText(miles)} miles. Open it under Sales › Invoices.`, action: { label: 'Go to Invoices', onClick: () => navigate?.('sales/invoices') } });
    } catch (err) {
      setFormError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex min-h-0 flex-1 flex-col">
      <div className="flex flex-none flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <label>
          <span className="qb-label">Customer:Job</span>
          <select className="qb-input w-[220px]" value={customerId} onChange={(e) => chooseCustomer(e.target.value)}>
            {jobs.length === 0 && <option value="">No unbilled mileage</option>}
            {jobs.map(([id, name]) => (
              <option key={id} value={id}>{name}</option>
            ))}
          </select>
        </label>
        <label>
          <span className="qb-label">Mileage item</span>
          <select className="qb-input w-[180px]" value={itemId} onChange={(e) => pickItem(e.target.value)}>
            <option value="">Choose an item…</option>
            {items.map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
        </label>
        <label>
          <span className="qb-label">Sales rate per mile</span>
          <input className="qb-input num w-[90px]" value={rate} onChange={(e) => setRate(e.target.value)} inputMode="decimal" />
        </label>
        <label>
          <span className="qb-label">Invoice date</span>
          <input type="date" className="qb-input w-[132px]" value={issue} onChange={(e) => setIssue(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Due date</span>
          <input type="date" className="qb-input w-[132px]" value={due} onChange={(e) => setDue(e.target.value)} />
        </label>
        <button type="button" className="qb-btn qb-btn-primary" onClick={bill} disabled={busy || chosen.length === 0}>
          Create invoice
        </button>
        <span className="ml-auto text-sm text-qb-ink-muted">
          {chosen.length} trip{chosen.length === 1 ? '' : 's'} · <span className="num font-semibold text-qb-ink">{milesText(miles)} mi</span>
          {estimate !== null && (
            <>
              {' '}· <span className="num font-semibold text-qb-ink">{formatCents(estimate)}</span>
            </>
          )}
        </span>
        {formError && <div className="w-full text-sm text-qb-negative">{formError}</div>}
      </div>
      {error && <Notice kind="error" text={`Could not load unbilled mileage: ${error}`} />}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head w-[32px]">
                <input
                  type="checkbox"
                  aria-label="Bill every trip"
                  checked={trips.length > 0 && chosen.length === trips.length}
                  onChange={(e) => setExcluded(new Set(e.target.checked ? [] : trips.map((t) => t.id)))}
                />
              </th>
              <th className="qb-grid-head">Date</th>
              <th className="qb-grid-head">Vehicle</th>
              <th className="qb-grid-head">Notes</th>
              <th className="qb-grid-head text-right">Miles</th>
            </tr>
          </thead>
          <tbody>
            {trips.length === 0 && (
              <tr>
                <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">Billable trips that aren’t on an invoice yet show here. Mark a trip Billable and give it a customer or job.</td>
              </tr>
            )}
            {trips.map((t) => (
              <tr key={t.id} className="qb-grid-row">
                <td className="qb-grid-cell">
                  <input
                    type="checkbox"
                    aria-label={`Bill the ${formatDate(t.tripDate)} trip`}
                    checked={!excluded.has(t.id)}
                    onChange={(e) =>
                      setExcluded((s) => {
                        const next = new Set(s);
                        if (e.target.checked) next.delete(t.id);
                        else next.add(t.id);
                        return next;
                      })
                    }
                  />
                </td>
                <td className="qb-grid-cell">{formatDate(t.tripDate)}</td>
                <td className="qb-grid-cell">{t.vehicleName}</td>
                <td className="qb-grid-cell text-qb-ink-muted">{t.notes ?? ''}</td>
                <td className="qb-grid-cell num font-semibold">{milesText(t.milesE4)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="flex-none border-t border-qb-border px-2 py-1 text-xs text-qb-ink-muted">
        Each trip becomes an invoice line: the miles as the quantity at the sales rate, posted to the item’s income account. Voiding the invoice makes the trips billable again.
      </div>
    </section>
  );
}

function VehiclesTab({ vehicles, onNotice }) {
  const [editing, setEditing] = useState(null); // null, 'new' or a vehicle
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [error, setError] = useState(null);
  const open = (v) => {
    setEditing(v);
    setName(v === 'new' ? '' : v.name);
    setDescription(v === 'new' ? '' : (v.description ?? ''));
    setError(null);
  };
  const save = async (vehicle, changes) => {
    try {
      const saved = await api.saveVehicle(vehicle?.id, { name: vehicle?.name, description: vehicle?.description ?? null, isActive: vehicle?.isActive ?? true, ...changes });
      setEditing(null);
      onNotice({ kind: 'success', text: `Saved ${saved.name}.` });
    } catch (err) {
      if (editing) setError(errorText(err));
      else onNotice({ kind: 'error', text: errorText(err) });
    }
  };
  return (
    <section className="qb-panel flex min-h-0 flex-1 flex-col">
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Vehicles</h2>
        <button type="button" className="qb-btn qb-btn-secondary h-row ml-auto" onClick={() => open('new')}>
          New vehicle
        </button>
      </div>
      {editing && (
        <div className="flex flex-none flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
          <label>
            <span className="qb-label">Vehicle name</span>
            <input autoFocus className="qb-input w-[200px]" value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="min-w-[220px] flex-1">
            <span className="qb-label">Description</span>
            <input className="qb-input w-full" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Year, make, model, plate" />
          </label>
          <button type="button" className="qb-btn qb-btn-primary" onClick={() => save(editing === 'new' ? null : editing, { name, description: description.trim() || null })}>
            Save
          </button>
          <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setEditing(null)}>
            Cancel
          </button>
          {error && <div className="w-full text-sm text-qb-negative">{error}</div>}
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Vehicle</th>
              <th className="qb-grid-head">Description</th>
              <th className="qb-grid-head text-right">Trips</th>
              <th className="qb-grid-head">Status</th>
              <th className="qb-grid-head" aria-label="Actions" />
            </tr>
          </thead>
          <tbody>
            {vehicles.length === 0 && (
              <tr>
                <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">No vehicles yet. Add each car or truck you record trips for.</td>
              </tr>
            )}
            {vehicles.map((v) => (
              <tr key={v.id} className="qb-grid-row">
                <td className="qb-grid-cell font-semibold">{v.name}</td>
                <td className="qb-grid-cell text-qb-ink-muted">{v.description ?? ''}</td>
                <td className="qb-grid-cell num">{v.tripCount}</td>
                <td className="qb-grid-cell">{v.isActive ? 'Active' : <span className="text-qb-ink-faint">Inactive</span>}</td>
                <td className="qb-grid-cell whitespace-nowrap text-right">
                  <button type="button" className="text-sm text-qb-link hover:underline" onClick={() => open(v)}>Edit</button>
                  <button type="button" className="ml-2 text-sm text-qb-link hover:underline" onClick={() => save(v, { isActive: !v.isActive })}>
                    {v.isActive ? 'Make inactive' : 'Make active'}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function RatesTab({ onNotice }) {
  const { data: rates, error } = useLedgerQuery(api.listMileageRates);
  const [date, setDate] = useState(todayIso());
  const [rate, setRate] = useState('');
  const [note, setNote] = useState('');
  const [formError, setFormError] = useState(null);
  const add = async () => {
    const rateE4 = parseQuantityE4(rate.replace(/^\$/, ''));
    if (rateE4 === null) return setFormError('Enter the rate in dollars per mile, like 0.70.');
    try {
      await api.saveMileageRate(null, { effectiveDate: date, rateE4, note: note.trim() || null });
      setRate('');
      setNote('');
      setFormError(null);
      onNotice({ kind: 'success', text: `Added ${rateText(rateE4)} from ${formatDate(date)}.` });
    } catch (err) {
      setFormError(errorText(err));
    }
  };
  const remove = async (r) => {
    try {
      await api.deleteMileageRate(r.id);
    } catch (err) {
      onNotice({ kind: 'error', text: errorText(err) });
    }
  };
  return (
    <section className="qb-panel flex min-h-0 flex-1 flex-col">
      <div className="flex-none border-b border-qb-border px-2 py-1 text-sm">
        <p>
          A trip’s deductible amount is its miles × the rate in effect on the trip date. LocalBooks doesn’t come with any rates: enter the official rate for each period yourself. The IRS
          publishes its standard mileage rates (business, medical/moving, charitable) at <span className="select-all font-semibold">irs.gov/tax-professionals/standard-mileage-rates</span>; check
          there or with your accountant when a new year or a mid-year change starts.
        </p>
      </div>
      <div className="flex flex-none flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <label>
          <span className="qb-label">Effective from</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Rate ($ per mile)</span>
          <input className="qb-input num w-[100px]" value={rate} onChange={(e) => setRate(e.target.value)} inputMode="decimal" placeholder="0.00" />
        </label>
        <label className="min-w-[200px] flex-1">
          <span className="qb-label">Note</span>
          <input className="qb-input w-full" value={note} onChange={(e) => setNote(e.target.value)} placeholder="IRS business rate" />
        </label>
        <button type="button" className="qb-btn qb-btn-primary" onClick={add}>
          Add rate
        </button>
        {formError && <div className="w-full text-sm text-qb-negative">{formError}</div>}
      </div>
      {error && <Notice kind="error" text={`Could not load rates: ${error}`} />}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Effective from</th>
              <th className="qb-grid-head text-right">Rate</th>
              <th className="qb-grid-head">Note</th>
              <th className="qb-grid-head" aria-label="Actions" />
            </tr>
          </thead>
          <tbody>
            {rates && rates.length === 0 && (
              <tr>
                <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">No rates yet. Add the rate that applies to your trips’ dates.</td>
              </tr>
            )}
            {(rates ?? []).map((r) => (
              <tr key={r.id} className="qb-grid-row">
                <td className="qb-grid-cell">{formatDate(r.effectiveDate)}</td>
                <td className="qb-grid-cell num font-semibold">{rateText(r.rateE4)}</td>
                <td className="qb-grid-cell text-qb-ink-muted">{r.note ?? ''}</td>
                <td className="qb-grid-cell text-right">
                  <button type="button" className="text-sm text-qb-link hover:underline" onClick={() => remove(r)}>Delete</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

export default function MileageTracking({ intent, navigate }) {
  const vehicles = useLedgerQuery(api.listVehicles).data ?? [];
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  const products = useLedgerQuery(api.listProducts).data ?? [];
  const [tab, setTab] = useState('trips');
  const [notice, setNotice] = useState(null);
  useEffect(() => {
    if (intent?.action === 'mileage-trip') setTab('trips');
  }, [intent]);

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="flex flex-none items-center gap-2">
        <div className="flex overflow-hidden rounded-xs border border-qb-border-strong text-sm" role="tablist" aria-label="Mileage">
          {TABS.map(([id, label]) => (
            <button key={id} type="button" role="tab" aria-selected={tab === id} onClick={() => setTab(id)} className={`px-2.5 py-0.5 font-semibold ${tab === id ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}>
              {label}
            </button>
          ))}
        </div>
        <button type="button" className="ml-auto text-sm text-qb-link hover:underline" onClick={() => navigate?.('reports', 'open', { report: 'mileage-vehicle' })}>
          Mileage reports
        </button>
      </div>
      {notice && (
        <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />
      )}
      {notice?.action && (
        <div>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={notice.action.onClick}>
            {notice.action.label}
          </button>
        </div>
      )}
      {tab === 'trips' && <TripsTab vehicles={vehicles} customers={customers} intent={intent} onNotice={setNotice} />}
      {tab === 'bill' && <BillTab products={products} onNotice={setNotice} navigate={navigate} />}
      {tab === 'vehicles' && <VehiclesTab vehicles={vehicles} onNotice={setNotice} />}
      {tab === 'rates' && <RatesTab onNotice={setNotice} />}
    </div>
  );
}
