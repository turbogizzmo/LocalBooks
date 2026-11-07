// What a new check, expense or bill fills in from its vendor:
// - My Preferences › General › "Recall a vendor's last transaction": `accounts` fills the lines'
//   accounts (and memos, classes and jobs) from the vendor's last check, expense or bill; `all`
//   fills their amounts too; `off` (the default) fills nothing.
// - Company Preferences › Checking › "Autofill payee account number in check memo".
// Neither touches a form whose lines were already typed in, or a transaction being edited.

import { useEffect, useRef } from 'react';
import { api } from './api.js';
import { centsToInput } from './money.js';
import { useMyPreferences } from './myPreferences.js';
import { useCompanyPreferences } from './companyPreferences.js';
import { blankSplit } from '../components/SplitGrid.jsx';

const pristine = (rows) => rows.every((r) => !r.amount && !r.qty && !r.rate && !r.memo);

/** Fills `rows` from the vendor's last transaction when the payee becomes a known vendor. */
export function useVendorRecall({ payee, vendors, rows, setRows, editing }) {
  const { recallVendorTransaction: mode } = useMyPreferences();
  const filledFor = useRef(null);
  const vendor = (vendors ?? []).find((v) => v.name.toLowerCase() === (payee ?? '').trim().toLowerCase());
  useEffect(() => {
    if (editing || mode === 'off' || !vendor || filledFor.current === vendor.id) return undefined;
    if (!pristine(rows) && filledFor.current === null) return undefined;
    let live = true;
    api.recallVendorTransaction(vendor.name).then(
      (last) => {
        if (!live || !last || last.lines.length === 0) return;
        filledFor.current = vendor.id;
        setRows(
          last.lines.map((l) => ({
            ...blankSplit(l.accountId),
            memo: l.memo ?? '',
            classId: l.classId ?? null,
            customerId: l.customerId ?? null,
            amount: mode === 'all' ? centsToInput(l.amountCents) : '',
          })),
        );
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [vendor?.id, mode, editing]); // eslint-disable-line react-hooks/exhaustive-deps
  /** Call when the form is cleared, so the next vendor fills again. */
  return () => {
    filledFor.current = null;
  };
}

/** The check memo from the vendor's account number, while the memo is blank or still the one filled in. */
export function useCheckMemo({ payee, vendors, memo, setMemo, enabled }) {
  const prefs = useCompanyPreferences();
  const filled = useRef('');
  const vendor = (vendors ?? []).find((v) => v.name.toLowerCase() === (payee ?? '').trim().toLowerCase());
  const on = enabled && prefs?.check_memo_vendor_account === 'true';
  useEffect(() => {
    if (!on) return;
    const next = vendor?.accountNumber ? `Acct ${vendor.accountNumber}` : '';
    if (memo === '' || memo === filled.current) {
      filled.current = next;
      setMemo(next);
    }
  }, [vendor?.id, vendor?.accountNumber, on]); // eslint-disable-line react-hooks/exhaustive-deps
}
