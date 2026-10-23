import { useEffect, useMemo, useRef, useState } from 'react';
import Notice from './Notice.jsx';
import ClassSelect, { useClassTracking } from './ClassSelect.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, formatMinutes, parseHoursToMinutes, parseMoneyToCents } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';

const DAY_NAMES = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

function mondayOf(iso) {
  const [y, m, d] = iso.split('-').map(Number);
  const weekday = (new Date(y, m - 1, d).getDay() + 6) % 7;
  return addDays(iso, -weekday);
}

function EmployeePicker({ employees, value, onChange, selectRef }) {
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState('');
  const [error, setError] = useState(null);

  const add = async () => {
    try {
      const created = await api.createEmployee(name.trim(), null);
      onChange(created.id);
      setAdding(false);
      setName('');
      setError(null);
    } catch (err) {
      setError(errorText(err));
    }
  };

  if (adding) {
    return (
      <span className="flex items-center gap-1">
        <input
          autoFocus
          className="qb-input w-[170px]"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              add();
            }
          }}
          placeholder="New employee name"
          aria-label="New employee name"
        />
        <button type="button" className="qb-btn qb-btn-primary" onClick={add} disabled={!name.trim()}>
          Add
        </button>
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setAdding(false)}>
          Cancel
        </button>
        {error && <span className="text-sm text-qb-negative">{error}</span>}
      </span>
    );
  }
  return (
    <select
      ref={selectRef}
      className="qb-input w-[200px]"
      value={value ?? ''}
      onChange={(e) => (e.target.value === '__new' ? setAdding(true) : onChange(e.target.value === '' ? null : Number(e.target.value)))}
    >
      <option value="">Choose an employee…</option>
      {employees.filter((e) => e.isActive).map((e) => (
        <option key={e.id} value={e.id}>{e.name}</option>
      ))}
      <option value="__new">＋ Add new employee</option>
    </select>
  );
}

function SingleActivity({ employees, customers, products, onSaved }) {
  const [employeeId, setEmployeeId] = useState(null);
  const [date, setDate] = useState(todayIso());
  const [customerId, setCustomerId] = useState(null);
  const [serviceId, setServiceId] = useState(null);
  const [billable, setBillable] = useState(false);
  const [rate, setRate] = useState('');
  const [hours, setHours] = useState('');
  const [description, setDescription] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const employeeRef = useRef(null);
  const tracking = useClassTracking();
  const [classId, setClassId] = useState(null);

  useEffect(() => employeeRef.current?.focus(), []);

  const minutes = parseHoursToMinutes(hours);
  const rateCents = rate.trim() === '' ? null : parseMoneyToCents(rate);

  const pickService = (id) => {
    setServiceId(id);
    const product = products.find((p) => p.id === id);
    if (product && product.rateCents > 0) setRate(centsToInput(product.rateCents));
  };

  const save = async () => {
    if (employeeId === null) return setError('Choose an employee.');
    if (minutes === null || minutes < 1 || minutes > 1440) return setError('Enter time as hours (1.5) or h:mm (1:30), up to 24 hours.');
    if (billable && (customerId === null || rateCents === null || rateCents < 0)) return setError('Billable time needs a customer and an hourly rate.');
    setBusy(true);
    try {
      await api.createTimeActivities([
        { employeeId, activityDate: date, customerId, productServiceId: serviceId, minutes, billable, hourlyRateCents: billable ? rateCents : null, description: description.trim() || null, classId },
      ]);
      onSaved(`Recorded ${formatMinutes(minutes)} for ${employees.find((e) => e.id === employeeId)?.name}.`);
      setHours('');
      setDescription('');
      setError(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="flex flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">Employee</span>
          <EmployeePicker employees={employees} value={employeeId} onChange={setEmployeeId} selectRef={employeeRef} />
        </label>
        <label>
          <span className="qb-label">Date</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Customer</span>
          <select className="qb-input w-[180px]" value={customerId ?? ''} onChange={(e) => setCustomerId(e.target.value === '' ? null : Number(e.target.value))}>
            <option value="">—</option>
            {customers.map((c) => (
              <option key={c.id} value={c.id}>{c.name}</option>
            ))}
          </select>
        </label>
        <label>
          <span className="qb-label">Service</span>
          <select className="qb-input w-[170px]" value={serviceId ?? ''} onChange={(e) => pickService(e.target.value === '' ? null : Number(e.target.value))}>
            <option value="">—</option>
            {products.filter((p) => p.isActive).map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
        </label>
        <label className="flex items-center gap-1 self-center pt-3 text-sm">
          <input type="checkbox" className="accent-qb-green" checked={billable} onChange={(e) => setBillable(e.target.checked)} />
          Billable
        </label>
        {billable && (
          <label>
            <span className="qb-label">Rate / hr</span>
            <input className="qb-input num w-[90px]" value={rate} onChange={(e) => setRate(e.target.value)} placeholder="0.00" inputMode="decimal" />
          </label>
        )}
        <label>
          <span className="qb-label">Time (hh:mm or 1.5)</span>
          <input className="qb-input num w-[90px]" value={hours} onChange={(e) => setHours(e.target.value)} placeholder="0:00" aria-invalid={hours !== '' && minutes === null} />
        </label>
        {tracking.enabled && tracking.hasClasses && (
          <label>
            <span className="qb-label">Class</span>
            <ClassSelect className="qb-input w-[160px]" classes={tracking.classes} value={classId} onChange={setClassId} />
          </label>
        )}
      </div>
      <div className="flex items-center gap-2 px-2 pb-1">
        <input className="qb-input flex-1" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Description of work" />
        {billable && minutes && rateCents !== null && (
          <span className="text-sm text-qb-ink-muted">
            {formatMinutes(minutes)} × {formatCents(rateCents)}/hr
          </span>
        )}
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
          {busy ? 'Saving…' : 'Save time'}
        </button>
      </div>
    </>
  );
}

let nextRow = 1;
const blankRow = () => ({ key: nextRow++, customerId: null, serviceId: null, billable: false, rate: '', description: '', cells: Array(7).fill('') });

function WeeklyTimesheet({ employees, customers, products, onSaved }) {
  const [employeeId, setEmployeeId] = useState(null);
  const [weekStart, setWeekStart] = useState(mondayOf(todayIso()));
  const [rows, setRows] = useState(() => [blankRow(), blankRow()]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const employeeRef = useRef(null);
  const tracking = useClassTracking();
  const [classId, setClassId] = useState(null);

  useEffect(() => employeeRef.current?.focus(), []);

  const days = Array.from({ length: 7 }, (_, i) => addDays(weekStart, i));
  const update = (key, patch) => setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...patch } : r)));
  const setCell = (key, day, value) => setRows((rs) => rs.map((r) => (r.key === key ? { ...r, cells: r.cells.map((c, i) => (i === day ? value : c)) } : r)));
  const minutesOf = (text) => (text.trim() === '' ? 0 : parseHoursToMinutes(text));
  const rowTotal = (r) => r.cells.reduce((s, c) => s + (minutesOf(c) ?? 0), 0);
  const dayTotal = (i) => rows.reduce((s, r) => s + (minutesOf(r.cells[i]) ?? 0), 0);
  const invalidCells = rows.flatMap((r) => r.cells.filter((c) => c.trim() !== '' && minutesOf(c) === null)).length;

  const save = async () => {
    if (employeeId === null) return setError('Choose an employee.');
    if (invalidCells > 0) return setError(`${invalidCells} time cell(s) are not valid (use 1.5 or 1:30).`);
    const inputs = [];
    for (const r of rows) {
      const rateCents = r.rate.trim() === '' ? null : parseMoneyToCents(r.rate);
      r.cells.forEach((cell, i) => {
        const minutes = minutesOf(cell);
        if (minutes) {
          inputs.push({
            employeeId,
            activityDate: days[i],
            customerId: r.customerId,
            productServiceId: r.serviceId,
            minutes,
            billable: r.billable,
            hourlyRateCents: r.billable ? rateCents : null,
            description: r.description.trim() || null,
            classId,
          });
        }
      });
      if (r.billable && rowTotal(r) > 0 && (r.customerId === null || rateCents === null)) return setError('Billable rows need a customer and an hourly rate.');
    }
    if (inputs.length === 0) return setError('Enter time for at least one day.');
    setBusy(true);
    try {
      const count = await api.createTimeActivities(inputs);
      const total = inputs.reduce((s, i) => s + i.minutes, 0);
      onSaved(`Saved ${count} time entr${count === 1 ? 'y' : 'ies'} (${formatMinutes(total)}) for the week of ${formatDate(weekStart)}.`);
      setRows([blankRow(), blankRow()]);
      setError(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="flex flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">Employee</span>
          <EmployeePicker employees={employees} value={employeeId} onChange={setEmployeeId} selectRef={employeeRef} />
        </label>
        <label>
          <span className="qb-label">Week starting (Monday)</span>
          <input type="date" className="qb-input w-[132px]" value={weekStart} onChange={(e) => e.target.value && setWeekStart(mondayOf(e.target.value))} />
        </label>
        {tracking.enabled && tracking.hasClasses && (
          <label>
            <span className="qb-label">Class</span>
            <ClassSelect className="qb-input w-[160px]" classes={tracking.classes} value={classId} onChange={setClassId} />
          </label>
        )}
        <div className="ml-auto text-right">
          <div className="qb-label">Week total</div>
          <div className="num text-2xl font-semibold leading-none">{formatMinutes(rows.reduce((s, r) => s + rowTotal(r), 0))}</div>
        </div>
      </div>
      <table className="qb-grid">
        <colgroup>
          <col className="w-[160px]" />
          <col className="w-[150px]" />
          <col className="w-[54px]" />
          <col className="w-[80px]" />
          <col />
          {days.map((d) => (
            <col key={d} className="w-[64px]" />
          ))}
          <col className="w-[64px]" />
          <col className="w-[28px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">Customer</th>
            <th className="qb-grid-head">Service</th>
            <th className="qb-grid-head text-center">Bill</th>
            <th className="qb-grid-head text-right">Rate</th>
            <th className="qb-grid-head">Description</th>
            {days.map((d, i) => (
              <th key={d} className="qb-grid-head text-right">
                {DAY_NAMES[i]} <span className="font-normal">{Number(d.slice(8))}</span>
              </th>
            ))}
            <th className="qb-grid-head text-right">Total</th>
            <th className="qb-grid-head" aria-label="Remove" />
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.key} className="qb-grid-row">
              <td className="qb-grid-cell-edit">
                <select value={r.customerId ?? ''} onChange={(e) => update(r.key, { customerId: e.target.value === '' ? null : Number(e.target.value) })} aria-label="Customer">
                  <option value="">—</option>
                  {customers.map((c) => (
                    <option key={c.id} value={c.id}>{c.name}</option>
                  ))}
                </select>
              </td>
              <td className="qb-grid-cell-edit">
                <select
                  value={r.serviceId ?? ''}
                  onChange={(e) => {
                    const id = e.target.value === '' ? null : Number(e.target.value);
                    const product = products.find((p) => p.id === id);
                    update(r.key, { serviceId: id, rate: product && product.rateCents > 0 ? centsToInput(product.rateCents) : r.rate });
                  }}
                  aria-label="Service"
                >
                  <option value="">—</option>
                  {products.filter((p) => p.isActive).map((p) => (
                    <option key={p.id} value={p.id}>{p.name}</option>
                  ))}
                </select>
              </td>
              <td className="qb-grid-cell text-center">
                <input type="checkbox" className="accent-qb-green" checked={r.billable} onChange={(e) => update(r.key, { billable: e.target.checked })} aria-label="Billable" />
              </td>
              <td className="qb-grid-cell-edit">
                <input className="num" value={r.rate} onChange={(e) => update(r.key, { rate: e.target.value })} disabled={!r.billable} placeholder={r.billable ? '0.00' : ''} aria-label="Hourly rate" />
              </td>
              <td className="qb-grid-cell-edit">
                <input value={r.description} onChange={(e) => update(r.key, { description: e.target.value })} aria-label="Description" />
              </td>
              {r.cells.map((cell, i) => (
                <td key={days[i]} className="qb-grid-cell-edit">
                  <input
                    className="num"
                    value={cell}
                    onChange={(e) => setCell(r.key, i, e.target.value)}
                    aria-invalid={cell.trim() !== '' && minutesOf(cell) === null}
                    aria-label={`${DAY_NAMES[i]} hours`}
                  />
                </td>
              ))}
              <td className="qb-grid-cell num font-semibold">{rowTotal(r) ? formatMinutes(rowTotal(r)) : ''}</td>
              <td className="qb-grid-cell cell-flush text-center">
                <button type="button" onClick={() => setRows((rs) => (rs.length > 1 ? rs.filter((x) => x.key !== r.key) : [blankRow()]))} className="h-row w-full text-qb-ink-faint hover:text-qb-negative" aria-label="Remove row">
                  ×
                </button>
              </td>
            </tr>
          ))}
          <tr className="bg-qb-row-header font-semibold">
            <td className="px-2 py-0.5" colSpan={5}>
              <button type="button" className="text-sm text-qb-info hover:underline" onClick={() => setRows((rs) => [...rs, blankRow()])}>
                ＋ Add row
              </button>
            </td>
            {days.map((d, i) => (
              <td key={d} className={`qb-grid-cell num ${dayTotal(i) > 1440 ? 'text-qb-negative' : ''}`}>{dayTotal(i) ? formatMinutes(dayTotal(i)) : ''}</td>
            ))}
            <td className="qb-grid-cell num">{formatMinutes(rows.reduce((s, r) => s + rowTotal(r), 0))}</td>
            <td />
          </tr>
        </tbody>
      </table>
      <div className="flex items-center gap-2 px-2 py-1">
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <button type="button" className="qb-btn qb-btn-primary ml-auto" onClick={save} disabled={busy}>
          {busy ? 'Saving…' : 'Save timesheet'}
        </button>
      </div>
    </>
  );
}

export default function TimeTracking({ intent }) {
  const employees = useLedgerQuery(api.listEmployees);
  const customers = useLedgerQuery(api.listCustomers);
  const products = useLedgerQuery(api.listProducts);
  const [mode, setMode] = useState(intent?.action === 'weekly-timesheet' ? 'weekly' : 'single');
  const [formKey, setFormKey] = useState(0);
  const [notice, setNotice] = useState(null);
  const [range, setRange] = useState(() => [addDays(todayIso(), -30), addDays(todayIso(), 7)]);
  const { data: activities, error, reload } = useLedgerQuery(() => api.listTimeActivities(range[0], range[1]));

  useEffect(() => {
    if (intent?.action === 'weekly-timesheet') setMode('weekly');
    if (intent?.action === 'time-activity') setMode('single');
    setFormKey((k) => k + 1);
  }, [intent]);
  useEffect(() => {
    reload();
  }, [range, reload]);

  const totals = useMemo(() => {
    const list = activities ?? [];
    return { all: list.reduce((s, a) => s + a.minutes, 0), billable: list.filter((a) => a.billable).reduce((s, a) => s + a.minutes, 0) };
  }, [activities]);
  const shared = { employees: employees.data ?? [], customers: customers.data ?? [], products: products.data ?? [], onSaved: (text) => setNotice({ kind: 'success', text }) };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <section className="qb-panel flex-none">
        <div className="flex items-center gap-2 border-b border-qb-border px-2 py-1">
          <div className="flex overflow-hidden rounded-xs border border-qb-border-strong text-sm" role="tablist" aria-label="Entry mode">
            {[
              ['single', 'Single time activity'],
              ['weekly', 'Weekly timesheet'],
            ].map(([id, label]) => (
              <button key={id} type="button" role="tab" aria-selected={mode === id} onClick={() => setMode(id)} className={`px-2.5 py-0.5 font-semibold ${mode === id ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}>
                {label}
              </button>
            ))}
          </div>
          <span className="text-xs text-qb-ink-muted">Time is non-posting; it records hours for payroll and billing</span>
        </div>
        {mode === 'single' ? <SingleActivity key={formKey} {...shared} /> : <WeeklyTimesheet key={formKey} {...shared} />}
      </section>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load time: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Time activities</h2>
          <input type="date" className="qb-input h-row w-[128px] text-sm" value={range[0]} onChange={(e) => setRange([e.target.value, range[1]])} aria-label="From" />
          <span className="text-sm text-qb-ink-muted">to</span>
          <input type="date" className="qb-input h-row w-[128px] text-sm" value={range[1]} onChange={(e) => setRange([range[0], e.target.value])} aria-label="To" />
          <span className="ml-auto text-sm text-qb-ink-muted">
            Total <span className="num font-semibold text-qb-ink">{formatMinutes(totals.all)}</span> · billable{' '}
            <span className="num font-semibold text-qb-ink">{formatMinutes(totals.billable)}</span>
          </span>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[92px]" />
              <col className="w-[160px]" />
              <col className="w-[170px]" />
              <col className="w-[150px]" />
              <col />
              <col className="w-[80px]" />
              <col className="w-[110px]" />
              <col className="w-[72px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Employee</th>
                <th className="qb-grid-head">Customer</th>
                <th className="qb-grid-head">Service</th>
                <th className="qb-grid-head">Description</th>
                <th className="qb-grid-head">Billable</th>
                <th className="qb-grid-head text-right">Rate</th>
                <th className="qb-grid-head text-right">Time</th>
              </tr>
            </thead>
            <tbody>
              {activities && activities.length === 0 && (
                <tr>
                  <td colSpan={8} className="qb-grid-cell text-qb-ink-muted">No time recorded in this range.</td>
                </tr>
              )}
              {(activities ?? []).map((a) => (
                <tr key={a.id} className="qb-grid-row">
                  <td className="qb-grid-cell">{formatDate(a.activityDate)}</td>
                  <td className="qb-grid-cell font-semibold">{a.employeeName}</td>
                  <td className="qb-grid-cell">{a.customerName ?? ''}</td>
                  <td className="qb-grid-cell">
                    {a.serviceName ?? ''}
                    {a.className && <span className="ml-1 text-xs text-qb-ink-muted">· {a.className}</span>}
                  </td>
                  <td className="qb-grid-cell text-qb-ink-muted" title={a.description ?? ''}>{a.description ?? ''}</td>
                  <td className="qb-grid-cell">{a.billable ? <span className="font-semibold text-qb-green-active">✓ Yes</span> : <span className="text-qb-ink-faint">No</span>}</td>
                  <td className="qb-grid-cell num">{a.hourlyRateCents !== null ? `${formatCents(a.hourlyRateCents)}/hr` : ''}</td>
                  <td className="qb-grid-cell num font-semibold">{formatMinutes(a.minutes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
