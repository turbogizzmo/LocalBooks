import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';

export const LEDGER_CHANGED = 'ledger-changed';
/** A different company file became the live one (double-click, File › Open), or none is (Close Company). */
export const COMPANY_FILE_CHANGED = 'company-file-changed';

/**
 * The screens' calls into the backend, one file per feature under `api/` (named like the backend's
 * `src-tauri/src/commands/` files). Each file default-exports its wrappers; a new feature adds a
 * file and touches nothing here.
 */
export const api = Object.assign({}, ...Object.values(import.meta.glob('./api/*.js', { eager: true, import: 'default' })));

/** Opens the system print dialog (Print / Save as PDF), falling back to the browser's. */
export async function printDocument() {
  try {
    await api.printReport();
  } catch {
    window.print();
  }
}

export const PAYMENT_METHODS = [
  { id: 'CHECK', label: 'Check' },
  { id: 'CASH', label: 'Cash' },
  { id: 'CREDIT_CARD', label: 'Credit card' },
  { id: 'ACH', label: 'ACH / bank transfer' },
  { id: 'WIRE', label: 'Wire' },
  { id: 'OTHER', label: 'Other' },
];

/** Backend errors look like "JE_UNBALANCED: total debits ..."; show the human part. */
export function errorText(err) {
  const text = typeof err === 'string' ? err : (err?.message ?? String(err));
  return text.replace(/^[A-Z][A-Z0-9_]*: /, '');
}

/**
 * Runs `fetcher` on mount and again whenever the backend reports a committed ledger write.
 * Out-of-order responses are discarded so a slow early request cannot overwrite fresher data.
 */
export function useLedgerQuery(fetcher) {
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  const fetcherRef = useRef(fetcher);
  const seq = useRef(0);
  fetcherRef.current = fetcher;

  const reload = useCallback(() => {
    const id = ++seq.current;
    return fetcherRef.current().then(
      (result) => {
        if (id !== seq.current) return;
        setData(result);
        setError(null);
      },
      (err) => {
        if (id !== seq.current) return;
        setError(errorText(err));
      },
    );
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten = null;
    reload();
    listen(LEDGER_CHANGED, () => reload()).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [reload]);

  return { data, error, reload };
}
