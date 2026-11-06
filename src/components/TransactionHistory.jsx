import { useEffect } from 'react';
import Notice from './Notice.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { transactionTarget } from '../lib/openTransaction.js';

const typeLabel = (t) => SOURCE_LABEL[t.sourceType] ?? t.sourceType;

/**
 * Transaction History (Ctrl+H): the transactions linked to one transaction, such as an invoice and
 * the payments that paid it, a payment and the deposit it went into, or a bill and its bill
 * payments (`registers/history.rs`). Double-click a linked transaction to open it.
 */
export default function TransactionHistory({ journalEntryId, onClose, onOpen }) {
  const { data, error } = useLedgerQuery(() => api.getTransactionHistory(journalEntryId));
  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);
  const open = (t) => {
    onOpen(...transactionTarget(t));
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/30" role="dialog" aria-modal="true" aria-label="Transaction History">
      <div className="qb-panel flex w-[640px] max-w-[95vw] flex-col p-3 shadow-panel">
        <h2 className="mb-1 text-md font-semibold">Transaction History</h2>
        {error && <Notice kind="error" text={error} />}
        {data && (
          <p className="mb-2 text-sm">
            <span className="font-semibold">{typeLabel(data)}</span> · entry #{data.entryNumber} · {formatDate(data.entryDate)}
            {data.memo ? <span className="text-qb-ink-muted"> · {data.memo}</span> : null}
            {data.isVoided && <span className="ml-1 text-2xs font-semibold uppercase text-qb-negative">Voided</span>}
          </p>
        )}
        <div className="max-h-[320px] min-h-[96px] overflow-auto border border-qb-border scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[130px]" />
              <col className="w-[120px]" />
              <col className="w-[92px]" />
              <col className="w-[60px]" />
              <col />
              <col className="w-[110px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">Linked as</th>
                <th className="qb-grid-head">Type</th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Entry</th>
                <th className="qb-grid-head">Memo</th>
                <th className="qb-grid-head text-right">Amount</th>
              </tr>
            </thead>
            <tbody>
              {data && data.links.length === 0 && (
                <tr>
                  <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">
                    Nothing is linked to this {typeLabel(data).toLowerCase()}. Invoices, payments, deposits, bills, bill payments and credits show what they were paid, deposited or applied with.
                  </td>
                </tr>
              )}
              {data?.links.map((t) => (
                <tr
                  key={`${t.relation}-${t.journalEntryId}`}
                  className={`qb-grid-row cursor-pointer ${t.isVoided || t.linkVoided ? 'text-qb-ink-faint' : ''}`}
                  onDoubleClick={() => open(t)}
                  title="Double-click to open it"
                >
                  <td className="qb-grid-cell">{t.relation}</td>
                  <td className="qb-grid-cell">
                    {typeLabel(t)}
                    {(t.isVoided || t.linkVoided) && <span className="ml-1 text-2xs font-semibold uppercase text-qb-negative">Voided</span>}
                  </td>
                  <td className="qb-grid-cell">{formatDate(t.entryDate)}</td>
                  <td className="qb-grid-cell">{t.entryNumber}</td>
                  <td className="qb-grid-cell truncate text-qb-ink-muted" title={t.memo ?? ''}>{t.memo ?? ''}</td>
                  <td className="qb-grid-cell num">{formatCents(t.amountCents)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="mt-2 flex justify-end gap-2">
          {data && (
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => open(data)}>
              Open {typeLabel(data).toLowerCase()}
            </button>
          )}
          <button type="button" className="qb-btn qb-btn-primary h-row" autoFocus onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
