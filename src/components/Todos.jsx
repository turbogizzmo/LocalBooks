import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { SortTh, useSort } from './SortHead.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';

/** Who is signed in and what they may do with to-dos. Without users, everyone may do everything. */
export function useTodoAccess() {
  const { data } = useLedgerQuery(api.getSignInState);
  const usersOn = Boolean(data?.usersEnabled);
  const me = data?.user ?? null;
  const viewOnly = usersOn && me?.role === 'VIEW_ONLY';
  return {
    usersOn,
    me,
    canEdit: !viewOnly,
    // A View-only user ticks off only the to-dos assigned to them.
    canComplete: (t) => !viewOnly || (me && t.assignedUserId === me.id),
  };
}

const EMPTY = { title: '', note: '', dueDate: '', party: '', link: '', assignedUserId: '' };

/** "c:12", "v:3" or "e:7": one customer, vendor or employee. */
const partyKey = (t) => (t.customerId ? `c:${t.customerId}` : t.vendorId ? `v:${t.vendorId}` : t.employeeId ? `e:${t.employeeId}` : '');

/**
 * Adds or edits a to-do. `todo` is the one being edited, or null; `preset` fills a new one
 * ({ customerId } or { vendorId }, from a customer or vendor).
 */
export function TodoDialog({ todo, preset, onClose, onSaved }) {
  const [form, setForm] = useState(() =>
    todo
      ? {
          title: todo.title,
          note: todo.note,
          dueDate: todo.dueDate ?? '',
          party: partyKey(todo),
          link: todo.linkKind ? `${todo.linkKind}:${todo.linkId}` : '',
          assignedUserId: todo.assignedUserId ?? '',
        }
      : { ...EMPTY, party: partyKey(preset ?? {}) },
  );
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  const vendors = useLedgerQuery(api.listVendors).data ?? [];
  const employees = useLedgerQuery(api.listEmployees).data ?? [];
  const assignees = useLedgerQuery(api.listTodoAssignees).data ?? [];
  const [links, setLinks] = useState([]);
  const [kind, id] = form.party ? form.party.split(':') : [null, null];

  useEffect(() => {
    if (kind !== 'c' && kind !== 'v') {
      setLinks([]);
      return undefined;
    }
    let live = true;
    api.listTodoLinks(kind === 'c' ? Number(id) : null, kind === 'v' ? Number(id) : null).then(
      (rows) => live && setLinks(rows),
      () => live && setLinks([]),
    );
    return () => {
      live = false;
    };
  }, [kind, id]);

  const set = (key) => (e) => setForm((f) => ({ ...f, [key]: e.target.value, ...(key === 'party' ? { link: '' } : {}) }));
  // Keep showing a name that is now inactive, if the to-do is already about it.
  const options = (list, prefix) => list.filter((p) => p.isActive || form.party === `${prefix}:${p.id}`);

  const submit = async (e) => {
    e.preventDefault();
    const [linkKind, linkId] = form.link ? form.link.split(':') : [null, null];
    setBusy(true);
    try {
      const saved = await api.saveTodo(todo?.id, {
        title: form.title,
        note: form.note,
        dueDate: form.dueDate || null,
        customerId: kind === 'c' ? Number(id) : null,
        vendorId: kind === 'v' ? Number(id) : null,
        employeeId: kind === 'e' ? Number(id) : null,
        linkKind,
        linkId: linkId ? Number(linkId) : null,
        assignedUserId: form.assignedUserId === '' ? null : Number(form.assignedUserId),
      });
      onSaved(saved);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Dialog title={todo ? 'Edit To Do' : 'New To Do'} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="grid grid-cols-2 gap-2">
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">To do</span>
            <input className="qb-input" value={form.title} onChange={set('title')} maxLength={200} placeholder="e.g. Call about the overdue invoice" />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Due date</span>
            <input type="date" className="qb-input" value={form.dueDate} onChange={set('dueDate')} />
          </label>
          {assignees.length > 0 ? (
            <label className="flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">Assigned to</span>
              <select className="qb-input" value={form.assignedUserId} onChange={set('assignedUserId')}>
                <option value="">Anyone</option>
                {assignees.map((u) => (
                  <option key={u.id} value={u.id}>{u.displayName}</option>
                ))}
              </select>
            </label>
          ) : (
            <span />
          )}
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">About (customer, vendor or employee)</span>
            <select className="qb-input" value={form.party} onChange={set('party')}>
              <option value="">Nobody in particular</option>
              <optgroup label="Customers">
                {options(customers, 'c').map((p) => <option key={`c${p.id}`} value={`c:${p.id}`}>{p.name}</option>)}
              </optgroup>
              <optgroup label="Vendors">
                {options(vendors, 'v').map((p) => <option key={`v${p.id}`} value={`v:${p.id}`}>{p.name}</option>)}
              </optgroup>
              <optgroup label="Employees">
                {options(employees, 'e').map((p) => <option key={`e${p.id}`} value={`e:${p.id}`}>{p.name}</option>)}
              </optgroup>
            </select>
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">{kind === 'v' ? 'Bill' : 'Invoice'}</span>
            <select className="qb-input" value={form.link} onChange={set('link')} disabled={kind !== 'c' && kind !== 'v'}>
              <option value="">{kind === 'c' || kind === 'v' ? 'None' : 'Choose a customer or vendor first'}</option>
              {form.link && !links.some((l) => `${l.kind}:${l.id}` === form.link) && <option value={form.link}>{todo?.linkLabel ?? 'Linked document'}</option>}
              {links.map((l) => (
                <option key={`${l.kind}${l.id}`} value={`${l.kind}:${l.id}`}>{l.label}</option>
              ))}
            </select>
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Note</span>
            <textarea className="qb-input h-[72px] py-1" value={form.note} onChange={set('note')} maxLength={4000} />
          </label>
        </div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>Save</button>
        </div>
      </form>
    </Dialog>
  );
}

const about = (t) => [t.customerName, t.vendorName, t.employeeName].find(Boolean) ?? '';

/** Whether an open to-do's due date passes the Due filter. */
function dueMatches(filter, t, today) {
  if (filter === 'all') return true;
  if (filter === 'none') return !t.dueDate;
  if (!t.dueDate) return false;
  if (filter === 'overdue') return t.dueDate < today;
  if (filter === 'today') return t.dueDate <= today;
  return t.dueDate <= addDays(today, 7);
}

/** Accounting › To Do List: tasks with due dates, about customers, vendors, employees or a document. */
export default function Todos({ intent, navigate }) {
  const { data, error } = useLedgerQuery(api.listTodos);
  const access = useTodoAccess();
  const [editing, setEditing] = useState(null);
  const [notice, setNotice] = useState(null);
  const [status, setStatus] = useState('open');
  const [due, setDue] = useState('all');
  const [who, setWho] = useState('all');
  const [query, setQuery] = useState('');
  const today = todayIso();

  // + New › To Do, or New to-do… on a customer or vendor.
  useEffect(() => {
    if (intent?.action === 'todo') setEditing({ todo: null, preset: intent.data ?? null });
  }, [intent]);

  const q = query.trim().toLowerCase();
  const rows = (data ?? []).filter(
    (t) =>
      (status === 'all' || (status === 'done') === t.isDone) &&
      dueMatches(due, t, today) &&
      (who === 'all' || (who === 'me' ? t.assignedUserId === access.me?.id : who === 'none' ? !t.assignedUserId : String(t.assignedUserId) === who)) &&
      (!q || [t.title, t.note, about(t), t.linkLabel].some((s) => s?.toLowerCase().includes(q))),
  );
  const sorter = useSort(rows, {
    done: (t) => (t.isDone ? 1 : 0),
    title: (t) => t.title,
    due: (t) => t.dueDate,
    about: (t) => about(t),
    assigned: (t) => t.assignedName,
  }, { storageKey: 'todos' });
  const assignees = useLedgerQuery(api.listTodoAssignees).data ?? [];

  const toggle = async (t) => {
    try {
      await api.setTodoDone(t.id, !t.isDone);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {editing && (
        <TodoDialog
          todo={editing.todo}
          preset={editing.preset}
          onClose={() => setEditing(null)}
          onSaved={(t) => {
            setEditing(null);
            setNotice({ kind: 'success', text: `Saved “${t.title}”.` });
          }}
        />
      )}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2 text-sm">
          <h2 className="text-md font-semibold">To Do List</h2>
          <input className="qb-input h-row w-[180px]" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Find a to-do" aria-label="Find a to-do" />
          <select className="qb-input h-row" value={status} onChange={(e) => setStatus(e.target.value)} aria-label="Status">
            <option value="open">Not done</option>
            <option value="done">Done</option>
            <option value="all">All</option>
          </select>
          <select className="qb-input h-row" value={due} onChange={(e) => setDue(e.target.value)} aria-label="Due">
            <option value="all">Any due date</option>
            <option value="overdue">Overdue</option>
            <option value="today">Due today or earlier</option>
            <option value="week">Due within 7 days</option>
            <option value="none">No due date</option>
          </select>
          {access.usersOn && (
            <select className="qb-input h-row" value={who} onChange={(e) => setWho(e.target.value)} aria-label="Assigned to">
              <option value="all">Everyone’s</option>
              <option value="me">Mine</option>
              <option value="none">Unassigned</option>
              {assignees.map((u) => (
                <option key={u.id} value={String(u.id)}>{u.displayName}</option>
              ))}
            </select>
          )}
          <span className="ml-auto text-qb-ink-muted">{rows.length} shown</span>
          {access.canEdit && (
            <button type="button" className="qb-btn qb-btn-primary h-row px-2" onClick={() => setEditing({ todo: null, preset: null })}>
              New To Do
            </button>
          )}
        </div>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {error && <Notice kind="error" text={error} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[44px]" />
              <col />
              <col className="w-[104px]" />
              <col className="w-[220px]" />
              {access.usersOn && <col className="w-[140px]" />}
              <col className="w-[48px]" />
            </colgroup>
            <thead>
              <tr>
                <SortTh sorter={sorter} col="done">Done</SortTh>
                <SortTh sorter={sorter} col="title">To do</SortTh>
                <SortTh sorter={sorter} col="due">Due</SortTh>
                <SortTh sorter={sorter} col="about">About</SortTh>
                {access.usersOn && <SortTh sorter={sorter} col="assigned">Assigned to</SortTh>}
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={access.usersOn ? 6 : 5} className="qb-grid-cell text-qb-ink-muted">
                    {(data ?? []).length === 0 ? 'No to-dos yet. Add one with New To Do, or from + New › To Do.' : 'No to-dos match these filters.'}
                  </td>
                </tr>
              )}
              {sorter.sorted.map((t) => {
                const overdue = !t.isDone && t.dueDate && t.dueDate < today;
                return (
                  <tr key={t.id} className={`qb-grid-row ${t.isDone ? 'text-qb-ink-faint' : ''}`} onDoubleClick={() => access.canEdit && setEditing({ todo: t })}>
                    <td className="qb-grid-cell text-center">
                      <input
                        type="checkbox"
                        className="accent-qb-green"
                        checked={t.isDone}
                        disabled={!access.canComplete(t)}
                        title={access.canComplete(t) ? undefined : 'View only: you can complete only the to-dos assigned to you'}
                        onChange={() => toggle(t)}
                        aria-label={`${t.isDone ? 'Reopen' : 'Complete'} ${t.title}`}
                      />
                    </td>
                    <td className={`qb-grid-cell ${t.isDone ? 'line-through' : ''}`} title={t.note || t.title}>
                      {t.title}
                      {t.note && <span className="ml-1 text-qb-ink-muted">— {t.note}</span>}
                    </td>
                    <td className={`qb-grid-cell ${overdue ? 'font-semibold text-qb-negative' : ''}`}>{formatDate(t.dueDate)}</td>
                    <td className="qb-grid-cell text-qb-ink-muted" title={[about(t), t.linkLabel].filter(Boolean).join(' · ')}>
                      {[about(t), t.linkLabel].filter(Boolean).join(' · ')}
                    </td>
                    {access.usersOn && <td className="qb-grid-cell text-qb-ink-muted">{t.assignedName ?? 'Anyone'}</td>}
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        label={`Actions for ${t.title}`}
                        items={[
                          ...(access.canEdit ? [{ label: 'Edit…', onSelect: () => setEditing({ todo: t }) }] : []),
                          ...(t.linkEntryId ? [{ label: `Open ${t.linkLabel ?? 'linked document'}`, onSelect: () => navigate('accounting/journal', 'open-entry', { entryId: t.linkEntryId }) }] : []),
                          { label: t.isDone ? 'Mark not done' : 'Mark done', disabled: !access.canComplete(t), onSelect: () => toggle(t) },
                          ...(access.canEdit
                            ? [{
                                label: 'Delete…',
                                danger: true,
                                onSelect: () => api.deleteTodo(t.id),
                                confirm: { title: 'Delete this to-do?', body: `“${t.title}” is removed from the To Do list.`, confirmLabel: 'Delete' },
                              }]
                            : []),
                        ]}
                      />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
