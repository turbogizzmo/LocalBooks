import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { PartyDialog } from './PartyList.jsx';
import { api, errorText } from '../lib/api.js';

/**
 * "<name> isn't on your customer list." — asked when a form is saved, or its name box is left,
 * with a customer or vendor name that is not on the list. Nothing adds a name without this
 * question, so a typo can't become a duplicate customer or vendor:
 *
 * - **Quick add** adds the name as it was typed.
 * - **Set up…** adds it and opens its record for the address, terms and the rest.
 * - **Cancel** adds nothing; the person goes back to the name box.
 *
 * The name box asks when it is left (`NameField`), and a save the backend refused because the
 * name is not on the list asks here too (`lib/confirmable.js`). When both happen at once (the
 * person typed a new name and clicked Save), they share one question.
 */

const create = { customer: (name) => api.createCustomer(name, null), vendor: (name) => api.createVendor(name, null) };

const pending = new Map();
const keyOf = (kind, name) => `${kind}:${name.trim().toLocaleLowerCase()}`;

/**
 * Asks about `name`; resolves `{ name }` (the name as saved) once it is on the list, or `null`
 * when the person cancelled. `start` skips the question when the person already answered it
 * (the name box's own "Add … as a new customer" and "Add with details…" rows): `quick` adds the
 * name, `details` adds it and opens its record.
 */
export function askNewName(kind, name, { start = 'ask' } = {}) {
  const key = keyOf(kind, name);
  if (pending.has(key)) return pending.get(key);
  const answer = show((done) => <NewNameDialog kind={kind} name={name.trim()} start={start} onDone={done} />).finally(() => pending.delete(key));
  pending.set(key, answer);
  return answer;
}

/** Puts the cursor back in the name box that holds `name`, after Cancel. */
export function focusNameBox(name) {
  const wanted = name.trim().toLocaleLowerCase();
  const boxes = [...document.querySelectorAll('input[data-name-box]')].filter((el) => el.isConnected && el.value.trim().toLocaleLowerCase() === wanted);
  const box = boxes[boxes.length - 1];
  if (box) {
    box.focus();
    box.select();
  }
}

function show(render) {
  return new Promise((resolve) => {
    const host = document.createElement('div');
    document.body.appendChild(host);
    const root = createRoot(host);
    const done = (value) => {
      root.unmount();
      host.remove();
      resolve(value ?? null);
    };
    root.render(render(done));
  });
}

function NewNameDialog({ kind, name, start, onDone }) {
  const noun = kind;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  // The record being set up: { id, name } once the name has been added.
  const [record, setRecord] = useState(null);
  const quickRef = useRef(null);
  const started = useRef(false);

  const add = async (thenSetUp) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const created = await create[kind](name);
      if (thenSetUp) setRecord({ id: created.id, name: created.name });
      else onDone({ name: created.name });
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const answered = start !== 'ask';
  useEffect(() => {
    if (answered && !started.current) {
      started.current = true;
      add(start === 'details');
    }
    // Once, on open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (record || answered) return undefined;
    quickRef.current?.focus();
    const onKey = (e) => {
      if (e.key === 'Escape') onDone(null);
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [record, answered, onDone]);

  if (record) {
    return (
      <PartyDialog
        kind={kind}
        id={record.id}
        // The name is on the list now, saved or not; the record keeps any rename.
        onClose={() => onDone({ name: record.name })}
        onSaved={(_text, saved) => onDone({ name: saved?.name ?? record.name })}
      />
    );
  }
  if (answered) {
    return error ? (
      <div className="fixed inset-0 z-[60] grid place-items-center bg-qb-nav/40">
        <div role="alertdialog" aria-modal="true" aria-labelledby="new-name-error" className="w-[420px] rounded-xs bg-white px-3 py-2 text-sm shadow-popover">
          <p id="new-name-error" className="text-qb-negative">{error}</p>
          <div className="mt-2 flex justify-end">
            <button type="button" className="qb-btn qb-btn-secondary" onClick={() => onDone(null)}>OK</button>
          </div>
        </div>
      </div>
    ) : null;
  }

  return (
    <div className="fixed inset-0 z-[60] grid place-items-center whitespace-normal bg-qb-nav/40 text-left backdrop-blur-[2px]">
      <div role="alertdialog" aria-modal="true" aria-labelledby="new-name-message" aria-describedby="new-name-hint" className="w-[440px] rounded-xs bg-white shadow-popover">
        <div className="space-y-1.5 px-3 py-2 text-sm">
          <p id="new-name-message" className="font-semibold">
            {name} isn&apos;t on your {noun} list.
          </p>
          <p id="new-name-hint" className="text-xs text-qb-ink-muted">
            Quick add puts it on the list as typed. Set up… adds it and opens its record. Cancel takes you back to the name box.
          </p>
          {error && <p className="text-xs text-qb-negative">{error}</p>}
        </div>
        <div className="flex justify-end gap-2 border-t border-qb-border px-3 py-1.5">
          <button type="button" className="qb-btn qb-btn-secondary" disabled={busy} onClick={() => onDone(null)}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-secondary" disabled={busy} onClick={() => add(true)}>
            Set up…
          </button>
          <button ref={quickRef} type="button" className="qb-btn qb-btn-primary" disabled={busy} onClick={() => add(false)}>
            Quick add
          </button>
        </div>
      </div>
    </div>
  );
}
