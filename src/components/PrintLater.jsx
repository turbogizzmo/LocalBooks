import { useEffect, useState } from 'react';
import { api, useLedgerQuery } from '../lib/api.js';

const PREF = { CHECK: 'print_checks_later', INVOICE: 'print_invoices_later' };

/**
 * The "Print later" choice on a new check or invoice. It starts from the last choice saved on that
 * form, and `remember(on)` keeps a changed choice as the new default.
 */
export function usePrintLater(kind, enabled = true) {
  const prefs = useLedgerQuery(api.getPreferences).data;
  const saved = prefs?.[PREF[kind]] === 'true';
  const [on, setOn] = useState(false);
  const [touched, setTouched] = useState(false);
  useEffect(() => {
    if (!touched) setOn(saved);
  }, [saved, touched]);
  const set = (value) => {
    setTouched(true);
    setOn(value);
  };
  const remember = (value) => {
    if (value !== saved) api.setPrintLaterDefault(kind, value).catch(() => {});
  };
  return { on: enabled && on, set, remember };
}

/** The checkbox itself. */
export function PrintLaterBox({ printLater, label = 'Print later' }) {
  return (
    <label className="flex items-center gap-1.5 self-center text-sm" title="Wait in File › Print Forms until you print a batch">
      <input type="checkbox" className="accent-qb-green" checked={printLater.on} onChange={(e) => printLater.set(e.target.checked)} />
      {label}
    </label>
  );
}
