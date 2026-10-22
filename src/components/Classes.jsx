import { useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';

/** Parent choices for `cls`: active classes other than itself and its own subclasses. */
function parentOptions(classes, cls) {
  const below = (c) => cls && (c.id === cls.id || c.fullName.startsWith(`${cls.fullName}:`));
  return classes.filter((c) => c.isActive && !below(c) && c.depth < 4);
}

function ParentSelect({ classes, cls, value, onChange, ...rest }) {
  return (
    <select value={value ?? ''} onChange={(e) => onChange(e.target.value === '' ? null : Number(e.target.value))} {...rest}>
      <option value="">— Top level —</option>
      {parentOptions(classes, cls).map((c) => (
        <option key={c.id} value={c.id}>
          {c.fullName}
        </option>
      ))}
    </select>
  );
}

/**
 * The class list. Classes tag lines on sales, purchases and journal entries with a department,
 * location or line of business; Profit & Loss by Class reports each one separately.
 */
export default function Classes() {
  const { data, error } = useLedgerQuery(api.listClasses);
  const prefs = useLedgerQuery(api.getPreferences).data;
  const tracking = prefs?.class_tracking === 'true';
  const classes = data ?? [];
  const [name, setName] = useState('');
  const [parentId, setParentId] = useState(null);
  const [editing, setEditing] = useState(null); // { id, name, parentId }
  const [showInactive, setShowInactive] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);

  const run = async (action, success) => {
    setBusy(true);
    try {
      const result = await action();
      setNotice({ kind: 'success', text: success(result) });
      return true;
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
      return false;
    } finally {
      setBusy(false);
    }
  };

  const add = async (e) => {
    e.preventDefault();
    if (!name.trim()) return setNotice({ kind: 'error', text: 'Enter a class name.' });
    if (await run(() => api.createClass({ name: name.trim(), parentId }), (c) => `Added class ${c.fullName}.`)) {
      setName('');
      setParentId(null);
    }
  };

  const saveEdit = async (e) => {
    e.preventDefault();
    if (!editing.name.trim()) return setNotice({ kind: 'error', text: 'Enter a class name.' });
    if (await run(() => api.updateClass(editing.id, { name: editing.name.trim(), parentId: editing.parentId }), (c) => `Saved ${c.fullName}.`)) {
      setEditing(null);
    }
  };

  const setActive = (c, active) =>
    run(
      () => api.setClassActive(c.id, active),
      () => (active ? `${c.fullName} is active again.` : `${c.fullName} is inactive${classes.some((s) => s.parentId === c.id) ? ', with its subclasses' : ''}. Lines that already use it keep it.`),
    );

  const shown = classes.filter((c) => showInactive || c.isActive);
  const inactiveCount = classes.filter((c) => !c.isActive).length;

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Classes</h2>
          <span className="min-w-0 truncate text-xs text-qb-ink-muted">
            Tag lines on invoices, sales receipts, estimates, bills, expenses and journal entries by department, location or line of business. Reports › Profit &amp; Loss by Class shows each one.
          </span>
          <label className="ml-auto flex flex-none items-center gap-1 text-sm">
            <input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} />
            Show inactive{inactiveCount > 0 ? ` (${inactiveCount})` : ''}
          </label>
        </div>
        {prefs && (
          <div className={`flex flex-none items-center gap-2 border-b border-qb-border px-2 py-1 text-sm ${tracking ? 'bg-qb-green-tint' : 'bg-[#fff4e5]'}`}>
            <label className="flex items-center gap-1.5 font-semibold">
              <input
                type="checkbox"
                className="accent-qb-green"
                checked={tracking}
                onChange={(e) => run(() => api.setPreference('class_tracking', e.target.checked ? 'true' : 'false'), () => (e.target.checked ? 'Class tracking is on.' : 'Class tracking is off; existing classes stay on their transactions.'))}
              />
              Use class tracking
            </label>
            <span className="text-xs text-qb-ink-muted">
              {tracking
                ? 'A Class column shows on entry forms. Choose one class per transaction or a missing-class warning in Settings › Class tracking.'
                : 'Turn this on to show a Class column on entry forms; until then, classes you add here are not offered on forms.'}
            </span>
          </div>
        )}
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {error && <Notice kind="error" text={`Could not load classes: ${error}`} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col />
              <col className="w-[260px]" />
              <col className="w-[110px]" />
              <col className="w-[120px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">Class</th>
                <th className="qb-grid-head">Subclass of</th>
                <th className="qb-grid-head">Status</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              <tr className="bg-qb-row-selected">
                <td className="qb-grid-cell-edit">
                  <form id="new-class" onSubmit={add} />
                  <input form="new-class" value={name} onChange={(e) => setName(e.target.value)} placeholder="New class, e.g. East Region" aria-label="Class name" />
                </td>
                <td className="qb-grid-cell-edit">
                  <ParentSelect form="new-class" classes={classes} cls={null} value={parentId} onChange={setParentId} aria-label="Subclass of" />
                </td>
                <td className="qb-grid-cell" />
                <td className="qb-grid-cell cell-tight">
                  <button type="submit" form="new-class" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>
                    Add
                  </button>
                </td>
              </tr>
              {data && shown.length === 0 && (
                <tr>
                  <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">
                    No classes yet. Once you add one, a Class column appears on your sales, purchase and journal entry forms.
                  </td>
                </tr>
              )}
              {shown.map((c) =>
                editing?.id === c.id ? (
                  <tr key={c.id} className="qb-grid-row is-active">
                    <td className="qb-grid-cell-edit">
                      <form id="edit-class" onSubmit={saveEdit} />
                      <input
                        form="edit-class"
                        value={editing.name}
                        onChange={(e) => setEditing({ ...editing, name: e.target.value })}
                        onKeyDown={(e) => e.key === 'Escape' && setEditing(null)}
                        aria-label="Class name"
                        autoFocus
                      />
                    </td>
                    <td className="qb-grid-cell-edit">
                      <ParentSelect form="edit-class" classes={classes} cls={c} value={editing.parentId} onChange={(id) => setEditing({ ...editing, parentId: id })} aria-label="Subclass of" />
                    </td>
                    <td className="qb-grid-cell cell-tight">
                      <button type="button" className="qb-btn qb-btn-secondary h-[20px] w-full text-xs" onClick={() => setEditing(null)} disabled={busy}>
                        Cancel
                      </button>
                    </td>
                    <td className="qb-grid-cell cell-tight">
                      <button type="submit" form="edit-class" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>
                        Save
                      </button>
                    </td>
                  </tr>
                ) : (
                  <tr key={c.id} className={`qb-grid-row ${c.isActive ? '' : 'text-qb-ink-faint'}`}>
                    <td className="qb-grid-cell" title={c.fullName}>
                      <span style={{ paddingLeft: `${c.depth * 16}px` }} className={c.depth === 0 ? 'font-semibold' : ''}>
                        {c.name}
                      </span>
                    </td>
                    <td className="qb-grid-cell text-qb-ink-muted">{c.parentId ? classes.find((p) => p.id === c.parentId)?.fullName : ''}</td>
                    <td className="qb-grid-cell text-sm">{c.isActive ? 'Active' : 'Inactive'}</td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        label={`Actions for ${c.fullName}`}
                        items={[
                          { label: 'Edit', onSelect: () => setEditing({ id: c.id, name: c.name, parentId: c.parentId }) },
                          c.isActive
                            ? { label: 'Make inactive', onSelect: () => setActive(c, false) }
                            : { label: 'Make active', onSelect: () => setActive(c, true) },
                          {
                            label: 'Delete',
                            danger: true,
                            disabled: c.isUsed,
                            hint: c.isUsed ? 'Used on transactions: make it inactive instead' : undefined,
                            onSelect: () => api.deleteClass(c.id).then(() => setNotice({ kind: 'success', text: `Deleted class ${c.fullName}.` })),
                            confirm: { title: `Delete ${c.fullName}?`, body: 'This class has never been used, so deleting it changes no transactions.', confirmLabel: 'Delete class' },
                          },
                        ]}
                      />
                    </td>
                  </tr>
                ),
              )}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
