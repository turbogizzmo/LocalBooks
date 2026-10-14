import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatMoney, formatRate, parseRateE6 } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

/** The last day of the month before `iso` (a typical period end). */
function lastMonthEnd(iso) {
  const d = new Date(`${iso.slice(0, 7)}-01T00:00:00`);
  d.setDate(0);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** Period-end revaluation of open foreign-currency invoices (unrealized gains and losses). */
export default function FxRevaluation() {
  const { homeCurrency } = useCompany();
  const history = useLedgerQuery(api.listFxRevaluations);
  const [asOf, setAsOf] = useState(() => lastMonthEnd(todayIso()));
  const [rates, setRates] = useState({});
  const [preview, setPreview] = useState(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);

  const parsed = useMemo(() => Object.fromEntries(Object.entries(rates).map(([c, t]) => [c, parseRateE6(t)]).filter(([, r]) => r !== null)), [rates]);
  const key = JSON.stringify([asOf, parsed]);

  useEffect(() => {
    let stale = false;
    const t = setTimeout(() => {
      api.previewFxRevaluation(asOf, JSON.parse(key)[1]).then(
        (p) => !stale && setPreview(p),
        (err) => !stale && setNotice({ kind: 'error', text: errorText(err) }),
      );
    }, 150);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [asOf, key]);

  const currencies = useMemo(() => [...new Set((preview?.lines ?? []).map((l) => l.currencyCode))], [preview]);
  const ready = preview && preview.lines.length > 0 && preview.missingRates.length === 0 && preview.netChangeCents !== 0;

  const post = async () => {
    const change = preview.netChangeCents;
    if (
      !window.confirm(
        `Post an unrealized ${change > 0 ? 'gain' : 'loss'} of ${formatCents(Math.abs(change))} on ${formatDate(asOf)}, reversed automatically the next day?`,
      )
    )
      return;
    setBusy(true);
    try {
      const r = await api.postFxRevaluation(asOf, parsed);
      setNotice({ kind: 'success', text: `Posted revaluation entry #${r.entryNumber} and its reversal #${r.reversalEntryNumber}.` });
      setPreview(await api.previewFxRevaluation(asOf, parsed));
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const undo = async (r) => {
    if (!window.confirm(`Void the revaluation as of ${formatDate(r.asOf)} and its reversal?`)) return;
    try {
      await api.voidFxRevaluation(r.id);
      setNotice({ kind: 'success', text: `Revaluation as of ${formatDate(r.asOf)} voided.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 overflow-auto p-2 scrollbar-thin">
      <section className="qb-panel">
        <div className="flex items-center gap-2 border-b border-qb-border px-3 py-2">
          <h2 className="text-md font-semibold">Currency revaluation</h2>
          <span className="text-xs text-qb-ink-muted">Open foreign-currency invoices at period-end rates</span>
        </div>
        <p className="border-b border-qb-border px-3 py-1.5 text-xs text-qb-ink-muted">
          Restates what customers owe in other currencies at the rates you enter, posting the unrealized gain or loss on the date
          chosen and reversing it the next day. The period’s Balance Sheet and Profit and Loss then reflect period-end rates, while
          payments still record their realized gain or loss against each invoice’s original rate.
        </p>
        <div className="flex flex-wrap items-end gap-3 px-3 py-2">
          <label className="flex flex-col gap-0.5 text-xs">
            <span className="font-semibold text-qb-ink-muted">Revalue as of</span>
            <input type="date" className="qb-input" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
          </label>
          {currencies.map((code) => (
            <label key={code} className="flex w-[150px] flex-col gap-0.5 text-xs">
              <span className="font-semibold text-qb-ink-muted">1 {code} = ? {homeCurrency}</span>
              <input
                className="qb-input num"
                value={rates[code] ?? ''}
                onChange={(e) => setRates((r) => ({ ...r, [code]: e.target.value }))}
                inputMode="decimal"
                placeholder="1.000000"
                aria-invalid={rates[code] !== undefined && rates[code] !== '' && parseRateE6(rates[code]) === null}
              />
            </label>
          ))}
          {preview && preview.lines.length === 0 && <span className="pb-1 text-sm text-qb-ink-muted">No foreign-currency invoices were open on that date.</span>}
        </div>
      </section>

      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}

      {preview && preview.lines.length > 0 && (
        <section className="qb-panel">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head">Invoice</th>
                <th className="qb-grid-head">Customer</th>
                <th className="qb-grid-head text-right">Open</th>
                <th className="qb-grid-head text-right">Booked rate</th>
                <th className="qb-grid-head text-right">Carried at</th>
                <th className="qb-grid-head text-right">At new rate</th>
                <th className="qb-grid-head text-right">Gain / (loss)</th>
              </tr>
            </thead>
            <tbody>
              {preview.lines.map((l) => (
                <tr key={l.invoiceId}>
                  <td className="qb-grid-cell">#{l.invoiceNumber}</td>
                  <td className="qb-grid-cell">{l.customerName}</td>
                  <td className="qb-grid-cell num">{formatMoney(l.foreignOpenCents, l.currencyCode)}</td>
                  <td className="qb-grid-cell num">{formatRate(l.bookedRateE6)}</td>
                  <td className="qb-grid-cell num">{formatCents(l.carryingCents)}</td>
                  <td className="qb-grid-cell num">{l.revaluedCents !== null ? formatCents(l.revaluedCents) : '—'}</td>
                  <td className={`qb-grid-cell num ${l.changeCents < 0 ? 'text-qb-negative' : ''}`}>{l.changeCents !== null ? formatCents(l.changeCents) : '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="flex items-center gap-3 border-t border-qb-border px-3 py-2">
            <span className="text-sm">
              Net unrealized {preview.netChangeCents >= 0 ? 'gain' : 'loss'}: <strong className="num">{formatCents(Math.abs(preview.netChangeCents))}</strong>
            </span>
            {preview.missingRates.length > 0 && <span className="text-xs text-qb-ink-muted">Enter a rate for {preview.missingRates.join(', ')}.</span>}
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row-lg px-3" disabled={busy || !ready} onClick={post}>
              Post revaluation
            </button>
          </div>
        </section>
      )}

      {(history.data ?? []).length > 0 && (
        <section className="qb-panel">
          <h3 className="border-b border-qb-border px-3 py-1.5 text-sm font-semibold">Past revaluations</h3>
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head">As of</th>
                <th className="qb-grid-head">Rates</th>
                <th className="qb-grid-head text-right">Gain / (loss)</th>
                <th className="qb-grid-head">Entries</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {history.data.map((r) => (
                <tr key={r.id} className={r.isVoided ? 'text-qb-ink-faint line-through' : ''}>
                  <td className="qb-grid-cell">{formatDate(r.asOf)}</td>
                  <td className="qb-grid-cell">
                    {Object.entries(JSON.parse(r.rates))
                      .map(([c, v]) => `${c} ${formatRate(v)}`)
                      .join(', ')}
                  </td>
                  <td className="qb-grid-cell num">{formatCents(r.netChangeCents)}</td>
                  <td className="qb-grid-cell">#{r.entryNumber} · reversed by #{r.reversalEntryNumber}</td>
                  <td className="qb-grid-cell cell-tight">
                    {!r.isVoided && (
                      <button type="button" className="qb-btn qb-btn-secondary h-[20px] px-2 text-xs" onClick={() => undo(r)}>
                        Void
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </div>
  );
}
