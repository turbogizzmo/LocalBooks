import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import Notice from './Notice.jsx';

const MADE_AT = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeStyle: 'short' });
const label = (source) => source.toLowerCase().replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase());

/**
 * Closing Date Exceptions (`closing::exceptions`): every posting or void dated on or before the
 * closing date that was in force when it was made, with who made it. Shown under the closing date.
 */
export default function ClosingExceptions() {
  const { data, error } = useLedgerQuery(api.getClosingExceptions);
  if (error) return <Notice kind="error" text={error} />;
  if (!data) return null;
  return (
    <section className="border-t border-qb-border" aria-label="Closing Date Exceptions">
      <h3 className="px-2 pt-1 text-xs font-semibold">Closing Date Exceptions</h3>
      {data.length === 0 ? (
        <p className="px-2 pb-1 text-xs text-qb-ink-muted">Nothing has been posted or voided in a closed period.</p>
      ) : (
        <div className="max-h-[160px] overflow-auto scrollbar-thin">
          <table className="qb-grid w-full text-xs">
            <thead>
              <tr>
                <th className="qb-grid-head">Made</th>
                <th className="qb-grid-head">By</th>
                <th className="qb-grid-head">Action</th>
                <th className="qb-grid-head">Dated</th>
                <th className="qb-grid-head">Type</th>
                <th className="qb-grid-head">Memo</th>
                <th className="qb-grid-head text-right">Amount</th>
                <th className="qb-grid-head">Closed through</th>
              </tr>
            </thead>
            <tbody>
              {data.map((x) => (
                <tr key={x.auditId} className="qb-grid-row">
                  <td className="qb-grid-cell">{MADE_AT.format(new Date(x.madeAt))}</td>
                  <td className="qb-grid-cell">{x.madeBy ?? ''}</td>
                  <td className="qb-grid-cell">{x.action === 'VOID' ? 'Voided' : 'Posted'}</td>
                  <td className="qb-grid-cell">{formatDate(x.entryDate)}</td>
                  <td className="qb-grid-cell">{label(x.sourceType)}</td>
                  <td className="qb-grid-cell">{x.memo ?? ''}</td>
                  <td className="qb-grid-cell num">{formatCents(x.amountCents)}</td>
                  <td className="qb-grid-cell">{formatDate(x.closingDate)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
