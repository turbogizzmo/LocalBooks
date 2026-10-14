import { useEffect, useRef, useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

const blankMapping = (headers) => ({
  date: headers[0] ?? '',
  description: headers[1] ?? '',
  amount: headers[2] ?? null,
  moneyIn: null,
  moneyOut: null,
  dateOrder: 'MDY',
});

function ColumnSelect({ label, headers, value, onChange, allowNone = false }) {
  return (
    <label className="flex flex-col gap-0.5 text-xs">
      <span className="font-semibold text-qb-ink-muted">{label}</span>
      <select className="qb-input h-row w-full text-sm" value={value ?? ''} onChange={(e) => onChange(e.target.value || null)}>
        {allowNone && <option value="">—</option>}
        {headers.map((h) => (
          <option key={h} value={h}>
            {h}
          </option>
        ))}
      </select>
    </label>
  );
}

/**
 * Lets the user say which CSV columns hold the date, description and amount when LocalBooks cannot
 * tell (or cannot tell the day/month order). The preview re-reads the file with each choice, and
 * the layout is remembered for the next file with the same headers.
 */
export default function BankColumnMapping({ path, inspection, accountName, onCancel, onImport }) {
  const [mapping, setMapping] = useState(() => inspection.mapping ?? blankMapping(inspection.headers));
  const [layout, setLayout] = useState(() => (inspection.mapping?.moneyIn ? 'split' : 'signed'));
  // Until the first re-read arrives, show nothing rather than the "unrecognized" message.
  const [preview, setPreview] = useState(inspection.mapping ? inspection : null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const first = useRef(null);
  const fileName = path.split(/[\\/]/).pop();

  useEffect(() => first.current?.focus(), []);

  const effective =
    layout === 'signed'
      ? { ...mapping, moneyIn: null, moneyOut: null, amount: mapping.amount ?? inspection.headers[0] }
      : { ...mapping, amount: null, moneyIn: mapping.moneyIn ?? inspection.headers[0], moneyOut: mapping.moneyOut ?? inspection.headers[1] };
  const key = JSON.stringify(effective);

  // Re-read the file with the current choices (debounced) so the preview always matches.
  useEffect(() => {
    let stale = false;
    const t = setTimeout(async () => {
      try {
        const next = await api.inspectBankFile(path, JSON.parse(key));
        if (!stale) setPreview(next);
      } catch (err) {
        if (!stale) setError(errorText(err));
      }
    }, 150);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [path, key]);

  const set = (field) => (value) => setMapping((m) => ({ ...m, [field]: value }));
  const ok = preview && !preview.problem && preview.rowCount > 0;

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await onImport(effective);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-start justify-center bg-black/30 p-6" role="dialog" aria-modal="true" aria-label="Choose bank file columns">
      <div className="qb-panel flex max-h-full w-full max-w-[760px] flex-col shadow-lg">
        <div className="flex items-center gap-2 border-b border-qb-border px-3 py-2">
          <h2 className="text-md font-semibold">Which columns are which?</h2>
          <span className="truncate text-xs text-qb-ink-muted" title={path}>
            {fileName} → {accountName}
          </span>
        </div>
        <p className="border-b border-qb-border px-3 py-1.5 text-xs text-qb-ink-muted">
          {inspection.mapping
            ? 'Check the day/month order: every date in this file could be read either way.'
            : 'LocalBooks did not recognize these column names. Choose them once; files with the same columns will then import directly.'}
        </p>

        <div className="grid grid-cols-2 gap-3 px-3 py-2 sm:grid-cols-4">
          <label className="flex flex-col gap-0.5 text-xs">
            <span className="font-semibold text-qb-ink-muted">Date</span>
            <select ref={first} className="qb-input h-row w-full text-sm" value={mapping.date} onChange={(e) => set('date')(e.target.value)}>
              {inspection.headers.map((h) => (
                <option key={h} value={h}>
                  {h}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5 text-xs">
            <span className="font-semibold text-qb-ink-muted">Dates are written</span>
            <select className="qb-input h-row w-full text-sm" value={mapping.dateOrder} onChange={(e) => set('dateOrder')(e.target.value)}>
              <option value="MDY">Month first (MM/DD)</option>
              <option value="DMY">Day first (DD/MM)</option>
            </select>
          </label>
          <div className="col-span-2">
            <ColumnSelect label="Description" headers={inspection.headers} value={mapping.description} onChange={set('description')} />
          </div>
        </div>

        <fieldset className="flex flex-wrap items-end gap-3 border-y border-qb-border px-3 py-2 text-xs">
          <legend className="sr-only">Amounts</legend>
          <div className="flex flex-col gap-1">
            <span className="font-semibold text-qb-ink-muted">Amounts are in</span>
            <label className="flex items-center gap-1.5">
              <input type="radio" name="layout" checked={layout === 'signed'} onChange={() => setLayout('signed')} className="accent-qb-green" />
              One column (negative = money out)
            </label>
            <label className="flex items-center gap-1.5">
              <input type="radio" name="layout" checked={layout === 'split'} onChange={() => setLayout('split')} className="accent-qb-green" />
              Two columns (money in, money out)
            </label>
          </div>
          {layout === 'signed' ? (
            <div className="w-[220px]">
              <ColumnSelect label="Amount" headers={inspection.headers} value={effective.amount} onChange={set('amount')} />
            </div>
          ) : (
            <>
              <div className="w-[180px]">
                <ColumnSelect label="Money in (deposits, credits)" headers={inspection.headers} value={effective.moneyIn} onChange={set('moneyIn')} />
              </div>
              <div className="w-[180px]">
                <ColumnSelect label="Money out (withdrawals, debits)" headers={inspection.headers} value={effective.moneyOut} onChange={set('moneyOut')} />
              </div>
            </>
          )}
        </fieldset>

        <div className="min-h-0 flex-1 overflow-auto px-3 py-2 scrollbar-thin">
          <div className="mb-1 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">
            Preview{preview?.rowCount ? ` · first ${preview.preview.length} of ${preview.rowCount} transactions` : ''}
          </div>
          {!preview ? (
            <p className="py-2 text-xs text-qb-ink-muted">Reading the file…</p>
          ) : preview.problem ? (
            <Notice kind="error" text={errorText(preview.problem)} />
          ) : (
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head w-[110px]">Date</th>
                  <th className="qb-grid-head">Description</th>
                  <th className="qb-grid-head w-[120px] text-right">Amount</th>
                </tr>
              </thead>
              <tbody>
                {(preview?.preview ?? []).map((r) => (
                  <tr key={r.row}>
                    <td className="qb-grid-cell">{formatDate(r.date)}</td>
                    <td className="qb-grid-cell truncate" title={r.description}>{r.description}</td>
                    <td className={`qb-grid-cell num ${r.amountCents < 0 ? 'text-qb-negative' : ''}`}>{formatCents(r.amountCents)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        {error && (
          <div className="px-3 pb-2">
            <Notice kind="error" text={error} onDismiss={() => setError(null)} />
          </div>
        )}
        <div className="flex items-center gap-2 border-t border-qb-border px-3 py-2">
          <span className="text-xs text-qb-ink-muted">Money out should show as negative (red). LocalBooks remembers these columns.</span>
          <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row-lg px-3" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" onClick={submit} disabled={busy || !ok}>
            {busy ? 'Importing…' : `Import ${preview?.rowCount ?? 0} transactions`}
          </button>
        </div>
      </div>
    </div>
  );
}
