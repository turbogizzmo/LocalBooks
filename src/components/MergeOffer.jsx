import { useState } from 'react';
import { api, errorText } from '../lib/api.js';
import { formatCents } from '../lib/money.js';

const NOUNS = { CUSTOMER: 'customer', VENDOR: 'vendor', ITEM: 'item', ACCOUNT: 'account' };

/** What the merge did, for the dialog's caller to show. */
export function mergedText(kind, r) {
  const moved = kind === 'ACCOUNT' && r.balanceCents ? ` Its balance of ${formatCents(Math.abs(r.balanceCents))} moved to ${r.toName} in one journal entry.` : '';
  return `Merged ${r.fromName} into ${r.toName}: everything that named ${r.fromName} now names ${r.toName}, and ${r.fromName} is inactive.${moved}`;
}

/**
 * Renaming an entry to another's name offers to merge them. `check(name)` looks for that other
 * entry before saving: it returns true (and shows the offer) when there is one, so the dialog waits.
 * `panel` is the offer; Merge merges and calls `onMerged(text)`, Keep editing closes the offer.
 */
export function useMergeOffer(kind, id, currentName, onMerged) {
  const [target, setTarget] = useState(null); // [id, name]
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const noun = NOUNS[kind];
  const check = async (name) => {
    if (!id || !name.trim() || name.trim().toLowerCase() === (currentName ?? '').trim().toLowerCase()) return false;
    const found = await api.findMergeTarget(kind, id, name.trim());
    if (!found) return false;
    setError(null);
    setTarget(found);
    return true;
  };
  const merge = async () => {
    setBusy(true);
    try {
      const result = await api.mergeNames(kind, id, target[0]);
      setTarget(null);
      onMerged(mergedText(kind, result));
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  const panel = target && (
    <div role="alertdialog" aria-label={`Merge into ${target[1]}`} className="rounded-xs border border-qb-border-strong bg-qb-canvas px-2 py-1.5 text-sm">
      <p>
        There is already a {noun} named <b>{target[1]}</b>. Merge <b>{currentName}</b> into it? Everything that names {currentName}
        {kind === 'ACCOUNT' ? ' (items, settings, subaccounts, and its balance, by one journal entry)' : ' (its transactions, lists and memorized transactions)'} will name {target[1]} instead, and{' '}
        {currentName} becomes inactive. Posted transactions keep their amounts. This can't be undone.
      </p>
      {error && <p className="mt-1 font-semibold text-qb-negative">{error}</p>}
      <div className="mt-1 flex justify-end gap-2">
        <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setTarget(null)} disabled={busy}>
          Keep editing
        </button>
        <button type="button" className="qb-btn qb-btn-primary h-row" onClick={merge} disabled={busy}>
          {busy ? 'Merging…' : `Merge into ${target[1]}`}
        </button>
      </div>
    </div>
  );
  return { check, panel };
}
