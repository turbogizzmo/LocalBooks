import { useState } from 'react';
import { ExportMenu, plainAmount, saveText, saveXlsx, tableSheet, toCsv } from './ReportParts.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import Notice from './Notice.jsx';

const MADE_AT = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeStyle: 'short' });
const COLUMNS = ['Made', 'By', 'Action', 'Dated', 'Type', 'Memo', 'Amount', 'Closed through'].map((label) => ({ label, align: label === 'Amount' ? 'right' : undefined }));
const TITLE = 'Closing Date Exceptions';
const label = (source) => source.toLowerCase().replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase());

/**
 * Closing Date Exceptions (`closing::exceptions`): every posting or void dated on or before the
 * closing date that was in force when it was made, with who made it. Shown under the closing date.
 */
export default function ClosingExceptions() {
  const { data, error } = useLedgerQuery(api.getClosingExceptions);
  const company = useCompany();
  const [notice, setNotice] = useState(null);
  if (error) return <Notice kind="error" text={error} />;
  if (!data) return null;
  const plainRows = () =>
    data.map((x) => [x.madeAt, x.madeBy ?? '', x.action === 'VOID' ? 'Voided' : 'Posted', x.entryDate, label(x.sourceType), x.memo ?? '', plainAmount(x.amountCents), x.closingDate]);
  const csv = () => saveText(toCsv([[company.name], [TITLE], [], COLUMNS.map((c) => c.label), ...plainRows()]), 'Closing-Date-Exceptions.csv', setNotice);
  const xlsx = () => saveXlsx(tableSheet({ company: company.name, title: TITLE, columns: COLUMNS, rows: plainRows() }), 'Closing-Date-Exceptions.xlsx', setNotice);
  return (
    <section className="border-t border-qb-border" aria-label={TITLE}>
      <div className="flex items-center gap-2 px-2 pt-1">
        <h3 className="text-xs font-semibold">{TITLE}</h3>
        {data.length > 0 && (
          <div className="ml-auto">
            <ExportMenu onNotice={setNotice} onCsv={csv} onXlsx={xlsx} printable={false} />
          </div>
        )}
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
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
