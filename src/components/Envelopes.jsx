// Printed envelopes (envelopes.rs): the envelope layout in Settings › Custom Forms. Printing one is
// `useEnvelopePrinter` in FormPages.jsx.
import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { FormSheet } from './FormPages.jsx';
import { api, errorText } from '../lib/api.js';

export const ENVELOPE_SIZES = [
  ['NUMBER10', '#10 business (9½ × 4⅛ in)'],
  ['NUMBER9', '#9 (8⅞ × 3⅞ in)'],
  ['MONARCH', 'Monarch (7½ × 3⅞ in)'],
  ['DL', 'DL (220 × 110 mm)'],
  ['C5', 'C5 (229 × 162 mm)'],
];

const FONTS = [
  ['HELVETICA', 'Sans serif'],
  ['TIMES', 'Serif'],
  ['COURIER', 'Typewriter'],
];

/** Settings › Custom Forms › Envelope: size, font, return address and where the address sits. */
export function EnvelopeDesigner({ tabs }) {
  // `tabs(dirty)` draws the form tabs, which ask before leaving unsaved changes.
  const [saved, setSaved] = useState(null);
  const [draft, setDraft] = useState(null);
  const [preview, setPreview] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .getEnvelopeLayout()
      .then((l) => {
        setSaved(l);
        setDraft(l);
      })
      .catch((err) => setNotice({ kind: 'error', text: errorText(err) }));
  }, []);

  useEffect(() => {
    if (!draft) return undefined;
    let cancelled = false;
    const timer = setTimeout(() => {
      api
        .renderEnvelope(null, draft)
        .then((r) => !cancelled && setPreview(r))
        .catch((err) => !cancelled && setNotice({ kind: 'error', text: errorText(err) }));
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [draft]);

  const dirty = Boolean(draft && saved && JSON.stringify(draft) !== JSON.stringify(saved));
  const set = (key) => (value) => setDraft((d) => ({ ...d, [key]: value }));
  const nudge = (key) => (e) => {
    const n = Number.parseInt(e.target.value || '0', 10);
    set(key)(Number.isFinite(n) ? Math.max(-72, Math.min(72, n)) : 0);
  };
  const run = async (fn, message) => {
    setBusy(true);
    try {
      const stored = await fn();
      setSaved(stored);
      setDraft(stored);
      setNotice({ kind: 'success', text: message });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="flex flex-none flex-wrap items-center gap-2">
        {tabs(dirty)}
        <span className="text-xs text-qb-ink-muted">One envelope layout for the whole company, used by Print Envelope on forms and in the customer and vendor lists.</span>
        <span className="ml-auto flex gap-1">
          <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy || !dirty} onClick={() => setDraft(saved)}>
            Undo changes
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy} onClick={() => run(api.resetEnvelopeLayout, 'Restored the standard envelope.')}>
            Reset to standard
          </button>
          <button type="button" className="qb-btn qb-btn-primary h-row" disabled={busy || !dirty} onClick={() => run(() => api.saveEnvelopeLayout(draft), 'Envelope layout saved. Print Envelope now uses it.')}>
            Save envelope
          </button>
        </span>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {!draft ? (
        <div className="px-2 py-1 text-sm text-qb-ink-muted">Loading…</div>
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-[340px_minmax(0,1fr)] gap-2">
          <div className="qb-panel flex min-h-0 flex-col gap-2 overflow-auto p-2 scrollbar-thin">
            <label className="flex flex-col gap-0.5">
              <span className="qb-label">Envelope size</span>
              <select className="qb-input" value={draft.size} onChange={(e) => set('size')(e.target.value)}>
                {ENVELOPE_SIZES.map(([id, label]) => (
                  <option key={id} value={id}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="qb-label">Font</span>
              <select className="qb-input" value={draft.font} onChange={(e) => set('font')(e.target.value)}>
                {FONTS.map(([id, label]) => (
                  <option key={id} value={id}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-center gap-1.5 text-sm">
              <input type="checkbox" checked={draft.returnAddress} onChange={(e) => set('returnAddress')(e.target.checked)} />
              Print the company&apos;s return address (from Company details)
            </label>
            <div className="flex flex-col gap-0.5">
              <span className="qb-label">Move the address (points; 72 is an inch)</span>
              <div className="flex items-center gap-2 text-sm">
                <label className="flex items-center gap-1">
                  Right
                  <input className="qb-input num w-[64px]" inputMode="numeric" value={draft.nudgeRight} onChange={nudge('nudgeRight')} aria-label="Move the address right" />
                </label>
                <label className="flex items-center gap-1">
                  Down
                  <input className="qb-input num w-[64px]" inputMode="numeric" value={draft.nudgeDown} onChange={nudge('nudgeDown')} aria-label="Move the address down" />
                </label>
              </div>
              <p className="text-2xs text-qb-ink-faint">Use a minus sign to move it left or up, if your printer feeds envelopes off center.</p>
            </div>
          </div>
          <div className="qb-panel flex min-h-0 flex-col bg-qb-canvas">
            <div className="flex h-row flex-none items-center gap-2 border-b border-qb-border px-2 text-xs text-qb-ink-muted">
              <span className="font-semibold text-qb-ink">Preview</span>
              <span>Sample envelope</span>
              {dirty && <span className="font-semibold text-qb-warning-ink">Unsaved changes</span>}
            </div>
            <div className="min-h-0 flex-1 overflow-auto p-3 scrollbar-thin">
              <div className="mx-auto max-w-[720px]">{preview?.pages.map((page, i) => <FormSheet key={i} page={page} logo={null} className="w-full border border-qb-border shadow-md" />)}</div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
