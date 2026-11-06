import { useState } from 'react';
import { createPortal } from 'react-dom';
import Notice from './Notice.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';

/** The six kinds of payment the ledger records; every payment method is one of them. */
export const PAYMENT_KINDS = [
  { id: 'CHECK', label: 'Check' },
  { id: 'CASH', label: 'Cash' },
  { id: 'CREDIT_CARD', label: 'Credit card' },
  { id: 'ACH', label: 'ACH / bank transfer' },
  { id: 'WIRE', label: 'Wire' },
  { id: 'OTHER', label: 'Other' },
];

export const kindLabel = (kind) => PAYMENT_KINDS.find((k) => k.id === kind)?.label ?? kind;

/** "2" or "2.5" (percent) → parts per million, without floating point; null when not a percent under 100. */
export function parsePercentToPpm(text) {
  const t = String(text ?? '').trim().replace(/%$/, '').trim();
  if (t === '') return 0;
  const m = /^(\d{1,2})(?:\.(\d{0,4}))?$/.exec(t);
  if (!m) return null;
  return Number(m[1]) * 10_000 + Number((m[2] ?? '').padEnd(4, '0'));
}

/** Parts per million → "2" / "2.5" (percent). */
export function formatPpm(ppm) {
  const whole = Math.trunc(ppm / 10_000);
  const frac = String(ppm % 10_000).padStart(4, '0').replace(/0+$/, '');
  return frac ? `${whole}.${frac}` : String(whole);
}

/** "2% 10 Net 30" style summary of a term's numbers. */
export function termsSummary(t) {
  const net = t.netDays === 0 ? 'due on receipt' : `net ${t.netDays} days`;
  return t.discountPpm > 0 ? `${formatPpm(t.discountPpm)}% off within ${t.discountDays} days, ${net}` : net;
}

export const useTerms = () => useLedgerQuery(api.listTerms).data ?? [];
export const usePaymentMethods = () => useLedgerQuery(api.listPaymentMethods).data ?? [];
export function useProfileEntries(list) {
  return (useLedgerQuery(api.listProfileEntries).data ?? []).filter((e) => e.list === list);
}

/** The first active method of a kind: what a form starts on when it used to start on that kind. */
export const methodOfKind = (methods, kind) => methods.find((m) => m.isActive && m.kind === kind) ?? null;

const ADD = '__add';

function AddDialog({ title, onClose, onSubmit, children }) {
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const submit = async (e) => {
    e.preventDefault();
    // Rendered in a portal, but React still bubbles the submit to the form around the box.
    e.stopPropagation();
    setBusy(true);
    try {
      await onSubmit();
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };
  return createPortal(
    <Dialog title={title} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="grid grid-cols-2 gap-2">{children}</div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>Add</button>
        </div>
      </form>
    </Dialog>,
    document.body,
  );
}

/** Asks for a new term: name, net days, and an optional early-payment discount. */
export function NewTermsDialog({ onClose, onCreated }) {
  const [name, setName] = useState('');
  const [netDays, setNetDays] = useState('30');
  const [percent, setPercent] = useState('');
  const [discountDays, setDiscountDays] = useState('');
  const submit = async () => {
    const ppm = parsePercentToPpm(percent);
    if (ppm === null) throw new Error('Enter the discount as a percent under 100, such as 2 or 1.5.');
    const t = await api.createTerms({
      name,
      netDays: Number(netDays),
      discountPpm: ppm,
      discountDays: ppm > 0 && discountDays.trim() !== '' ? Number(discountDays) : null,
    });
    onCreated(t);
  };
  return (
    <AddDialog title="New term" onClose={onClose} onSubmit={submit}>
      <label className="col-span-2 flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Name</span>
        <input className="qb-input" value={name} maxLength={31} onChange={(e) => setName(e.target.value)} placeholder="e.g. 2% 10 Net 30" autoFocus />
      </label>
      <label className="flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Net due in (days)</span>
        <input className="qb-input num" value={netDays} inputMode="numeric" onChange={(e) => setNetDays(e.target.value)} />
      </label>
      <span />
      <label className="flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Discount % (optional)</span>
        <input className="qb-input num" value={percent} inputMode="decimal" onChange={(e) => setPercent(e.target.value)} placeholder="e.g. 2" />
      </label>
      <label className="flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">if paid within (days)</span>
        <input className="qb-input num" value={discountDays} inputMode="numeric" onChange={(e) => setDiscountDays(e.target.value)} placeholder="e.g. 10" />
      </label>
    </AddDialog>
  );
}

/** Asks for a new payment method: its name and which kind of payment it is. */
export function NewPaymentMethodDialog({ onClose, onCreated }) {
  const [name, setName] = useState('');
  const [kind, setKind] = useState('CREDIT_CARD');
  const submit = async () => onCreated(await api.createPaymentMethod({ name, kind }));
  return (
    <AddDialog title="New payment method" onClose={onClose} onSubmit={submit}>
      <label className="flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Name</span>
        <input className="qb-input" value={name} maxLength={31} onChange={(e) => setName(e.target.value)} placeholder="e.g. Visa" autoFocus />
      </label>
      <label className="flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Kind</span>
        <select className="qb-input" value={kind} onChange={(e) => setKind(e.target.value)}>
          {PAYMENT_KINDS.map((k) => (
            <option key={k.id} value={k.id}>{k.label}</option>
          ))}
        </select>
      </label>
    </AddDialog>
  );
}

/**
 * The Terms box: the active terms (and the one already chosen), "＋ Add new…" at the end.
 * `onChange` gets the term (or null), so the form can set the due date from its days.
 */
export function TermsSelect({ value, onChange, className = 'qb-input w-[140px]', allowNone = true, ariaLabel = 'Terms' }) {
  const terms = useTerms();
  const [adding, setAdding] = useState(false);
  const options = terms.filter((t) => t.isActive || t.id === value);
  const chosen = terms.find((t) => t.id === value);
  return (
    <>
      <select
        className={className}
        aria-label={ariaLabel}
        value={value ?? ''}
        title={chosen ? termsSummary(chosen) : undefined}
        onChange={(e) => {
          if (e.target.value === ADD) return setAdding(true);
          onChange(e.target.value === '' ? null : (terms.find((t) => t.id === Number(e.target.value)) ?? null));
        }}
      >
        {(allowNone || value == null) && <option value="" />}
        {options.map((t) => (
          <option key={t.id} value={t.id} title={termsSummary(t)}>{t.name}</option>
        ))}
        <option value={ADD}>＋ Add new…</option>
      </select>
      {adding && (
        <NewTermsDialog
          onClose={() => setAdding(false)}
          onCreated={(t) => {
            setAdding(false);
            onChange(t);
          }}
        />
      )}
    </>
  );
}

/** The payment method box: active methods by name, "＋ Add new…" at the end. `onChange` gets the method. */
export function PaymentMethodSelect({ value, onChange, className = 'qb-input w-[150px]', allowNone = false, id, ariaLabel = 'Payment method' }) {
  const methods = usePaymentMethods();
  const [adding, setAdding] = useState(false);
  const options = methods.filter((m) => m.isActive || m.id === value);
  return (
    <>
      <select
        id={id}
        className={className}
        aria-label={ariaLabel}
        value={value ?? ''}
        onChange={(e) => {
          if (e.target.value === ADD) return setAdding(true);
          onChange(e.target.value === '' ? null : (methods.find((m) => m.id === Number(e.target.value)) ?? null));
        }}
      >
        {(allowNone || value == null) && <option value="" />}
        {options.map((m) => (
          <option key={m.id} value={m.id}>{m.name}</option>
        ))}
        <option value={ADD}>＋ Add new…</option>
      </select>
      {adding && (
        <NewPaymentMethodDialog
          onClose={() => setAdding(false)}
          onCreated={(m) => {
            setAdding(false);
            onChange(m);
          }}
        />
      )}
    </>
  );
}

/**
 * A form's payment method: the chosen list entry, starting on the first method of `startKind`.
 * `payload()` gives what the backend takes (the kind and the method).
 */
export function usePaymentMethodChoice(startKind = 'CHECK', startId = null) {
  const methods = usePaymentMethods();
  const [chosenId, setChosenId] = useState(startId);
  const chosen = methods.find((m) => m.id === chosenId) ?? methodOfKind(methods, startKind);
  return {
    id: chosen?.id ?? null,
    kind: chosen?.kind ?? startKind,
    set: (m) => setChosenId(m?.id ?? null),
    reset: () => setChosenId(null),
    payload: () => ({ paymentMethod: chosen?.kind ?? startKind, paymentMethodId: chosen?.id ?? null }),
  };
}

/** A box for a customer, vendor or job type, or a customer message; "＋ Add new…" adds to its list. */
export function ProfileEntrySelect({ list, value, onChange, className = 'qb-input', ariaLabel, noun }) {
  const entries = useProfileEntries(list);
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState('');
  const options = entries.filter((e) => e.isActive || e.id === value);
  return (
    <>
      <select
        className={className}
        aria-label={ariaLabel}
        value={value ?? ''}
        onChange={(e) => {
          if (e.target.value === ADD) return setAdding(true);
          onChange(e.target.value === '' ? null : Number(e.target.value));
        }}
      >
        <option value="" />
        {options.map((e) => (
          <option key={e.id} value={e.id}>{e.name}</option>
        ))}
        <option value={ADD}>＋ Add new…</option>
      </select>
      {adding && (
        <AddDialog
          title={`New ${noun}`}
          onClose={() => setAdding(false)}
          onSubmit={async () => {
            const entry = await api.createProfileEntry(list, name);
            setAdding(false);
            setName('');
            onChange(entry.id);
          }}
        >
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">{noun[0].toUpperCase() + noun.slice(1)}</span>
            <input className="qb-input w-[320px]" value={name} maxLength={list === 'CUSTOMER_MESSAGE' ? 101 : 31} onChange={(e) => setName(e.target.value)} autoFocus />
          </label>
        </AddDialog>
      )}
    </>
  );
}
