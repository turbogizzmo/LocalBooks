import { useRef, useState } from 'react';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { askConfirm } from '../lib/dialogs.jsx';
import { numberTaken, readDocNumber } from '../lib/docNumbers.js';

/**
 * The number box on sales forms (Invoice #, Estimate #, Sale #, Credit # and Order #). A new form
 * shows the number it will get, kept current while other windows save; the person may type
 * another. Left as shown, the form takes the next number when it is saved.
 *
 * Numbers are used once per kind of form (`doc_numbers.rs`). Saving with a number that is already
 * used says what uses it and offers the next free number instead; Cancel goes back to the box.
 *
 * `kind`: INVOICE, ESTIMATE, SALES_RECEIPT, CREDIT_MEMO or SALES_ORDER. `noun`: "Invoice" and so on.
 */
export function useDocNumber(kind, noun) {
  const next = useLedgerQuery(() => api.getNextDocumentNumber(kind));
  // What was typed; null while the box shows the next number.
  const [typed, setTyped] = useState(null);
  const inputRef = useRef(null);
  const text = typed ?? (next.data == null ? '' : String(next.data));

  return {
    noun,
    inputRef,
    text,
    setText: setTyped,
    /** Back to the next number: a new form, a duplicate, or after saving. */
    reset: () => setTyped(null),
    /**
     * Before saving: `{ value }` is the number to save under (null takes the next one), `{ error }`
     * what to tell the person; `{ cancelled: true }` when they chose to change a number in use.
     */
    async resolve() {
      if (typed === null) return { value: null };
      const read = readDocNumber(typed, noun);
      if (read.error || read.value === null) return read;
      const problem = await api.getDocumentNumberProblem(kind, read.value);
      if (!problem) return read;
      const taken = numberTaken(problem);
      if (!taken) return { error: errorText(problem) };
      const ok = await askConfirm(`${taken.message}\n\nEach ${noun.toLowerCase()} number is used once. Use #${taken.next} instead?`, {
        confirmLabel: `Use #${taken.next}`,
      });
      if (!ok) {
        inputRef.current?.focus();
        inputRef.current?.select();
        return { cancelled: true };
      }
      setTyped(String(taken.next));
      return { value: taken.next };
    },
  };
}

/** The box itself. `fixed`: the number of a saved form being changed, shown and not editable. */
export default function DocNumberBox({ number, fixed = null, title }) {
  const label = `${number.noun} #`;
  return (
    <label>
      <span className="qb-label">{label}</span>
      <input
        ref={number.inputRef}
        className="qb-input w-[96px] text-right tabular-nums"
        inputMode="numeric"
        value={fixed ?? number.text}
        disabled={fixed !== null}
        title={title ?? (fixed !== null ? undefined : `The ${number.noun.toLowerCase()} number. Type another to use it instead.`)}
        onChange={(e) => number.setText(e.target.value)}
      />
    </label>
  );
}
