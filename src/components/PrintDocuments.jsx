import { createContext, useCallback, useContext, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { api, errorText, printDocument } from '../lib/api.js';
import { formatMoney, formatQuantity } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

/** Company identity for printed documents: name, contact lines and logo (data URL or null). */
export const CompanyContext = createContext({ name: 'LocalBooks', legalName: '', address: '', email: '', phone: '', ein: '', logo: null, homeCurrency: 'USD' });

export function useCompany() {
  return useContext(CompanyContext);
}

/** Renders printable content into #print-root, which is what the system print dialog prints. */
export function PrintPortal({ children }) {
  const target = document.getElementById('print-root');
  return target ? createPortal(children, target) : null;
}

const LONG_DATE = new Intl.DateTimeFormat('en-US', { month: 'long', day: 'numeric', year: 'numeric', timeZone: 'UTC' });
export const longDate = (iso) => (iso ? LONG_DATE.format(new Date(`${iso}T00:00:00Z`)) : '');

/** Company block on the left; logo pinned to the top-right with the document title beneath it. */
function DocHeader({ company, title, subtitle }) {
  const lines = [company.legalName && company.legalName !== company.name ? company.legalName : null, company.address, [company.phone, company.email].filter(Boolean).join(' · '), company.ein ? `EIN ${company.ein}` : null]
    .filter(Boolean)
    .join('\n');
  return (
    <header className="doc-header">
      <div className="doc-company">
        <div className="doc-company-name">{company.name}</div>
        {lines && <div className="doc-company-lines">{lines}</div>}
      </div>
      <div className="doc-brand">
        {company.logo && <img className="doc-logo" src={company.logo} alt={`${company.name} logo`} />}
        <div className="doc-title">{title}</div>
        {subtitle && <div className="doc-subtitle">{subtitle}</div>}
      </div>
    </header>
  );
}

export function InvoiceDocument({ detail, company }) {
  const { invoice, items, payments } = detail;
  // A foreign-currency invoice prints in the customer's currency, with its ISO code.
  const formatCents = (cents) => formatMoney(cents, invoice.currencyCode);
  const paid = payments.reduce((s, p) => s + p.amountCents, 0);
  const termsDays = Math.round((Date.parse(invoice.dueDate) - Date.parse(invoice.issueDate)) / 86_400_000);
  const terms = termsDays === 0 ? 'Due on receipt' : `Net ${termsDays}`;
  return (
    <article className="doc">
      {invoice.isVoided && <div className="doc-void">VOID</div>}
      <DocHeader company={company} title="Invoice" subtitle={`No. ${invoice.invoiceNumber}`} />
      <section className="doc-parties">
        <div>
          <div className="doc-label">Bill to</div>
          <div className="doc-bill-to">{invoice.customerName}</div>
        </div>
        <table className="doc-meta">
          <tbody>
            <tr>
              <td>Invoice date</td>
              <td>{formatDate(invoice.issueDate)}</td>
            </tr>
            <tr>
              <td>Terms</td>
              <td>{terms}</td>
            </tr>
            <tr>
              <td>Due date</td>
              <td>{formatDate(invoice.dueDate)}</td>
            </tr>
            <tr>
              <td>Balance due</td>
              <td>
                <strong>{formatCents(invoice.isVoided ? 0 : invoice.balanceCents)}</strong>
              </td>
            </tr>
            {invoice.currencyCode && (
              <tr>
                <td>Currency</td>
                <td>{invoice.currencyCode}</td>
              </tr>
            )}
          </tbody>
        </table>
      </section>

      <table className="doc-table">
        <colgroup>
          <col style={{ width: '0.35in' }} />
          <col />
          <col style={{ width: '0.7in' }} />
          <col style={{ width: '1.05in' }} />
          <col style={{ width: '1.15in' }} />
        </colgroup>
        <thead>
          <tr>
            <th className="doc-num">#</th>
            <th>Description</th>
            <th className="doc-num">Qty</th>
            <th className="doc-num">Rate</th>
            <th className="doc-num">Amount</th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <tr key={item.lineNumber}>
              <td className="doc-num">{item.lineNumber}</td>
              <td>{item.description || '—'}</td>
              <td className="doc-num">{formatQuantity(item.quantityE4)}</td>
              <td className="doc-num">{formatCents(item.rateCents)}</td>
              <td className="doc-num">{formatCents(item.amountCents)}</td>
            </tr>
          ))}
        </tbody>
      </table>

      <table className="doc-totals">
        <tbody>
          <tr>
            <td>Total</td>
            <td className="doc-num">{formatCents(invoice.totalCents)}</td>
          </tr>
          {payments.map((p) => (
            <tr key={p.id}>
              <td>Payment received {formatDate(p.paymentDate)}</td>
              <td className="doc-num">-{formatCents(p.amountCents)}</td>
            </tr>
          ))}
          <tr className="doc-grand">
            <td>Balance due</td>
            <td className="doc-num">{formatCents(invoice.isVoided ? 0 : invoice.totalCents - paid)}</td>
          </tr>
        </tbody>
      </table>

      <p className="doc-note">
        {invoice.isVoided
          ? 'This invoice has been voided and no payment is due.'
          : `Please reference invoice ${invoice.invoiceNumber} with your payment. Thank you for your business.`}
      </p>
    </article>
  );
}

const LINE_CLASS = { section: 'doc-line-section', account: '', subtotal: 'doc-line-subtotal', total: 'doc-line-total', net: 'doc-line-net' };

/**
 * A financial statement for print. With `columns` (e.g. Profit and Loss by Class), each line
 * carries `amountsCents`, one per column; otherwise a single Total column from `amountCents`.
 */
export function StatementDocument({ company, title, subtitle, lines, footer, columns = null }) {
  const formatCents = (cents) => formatMoney(cents, null);
  const heads = columns ?? ['Total'];
  const amounts = (line) => (columns ? line.amountsCents : [line.amountCents]);
  return (
    <article className="doc">
      <DocHeader company={company} title={title} subtitle={subtitle} />
      <table className="doc-table doc-statement">
        <colgroup>
          <col />
          {heads.map((h) => (
            <col key={h} style={{ width: columns ? '1.05in' : '1.6in' }} />
          ))}
        </colgroup>
        <thead>
          <tr>
            <th />
            {heads.map((h) => (
              <th key={h} className="doc-num">{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {lines.map((line, i) => (
            <tr key={`${i}-${line.label}`} className={LINE_CLASS[line.kind]}>
              <td style={{ paddingLeft: `${6 + line.depth * 14}pt` }}>{line.label}</td>
              {amounts(line).map((cents, c) => (
                <td key={c} className="doc-num">{cents !== null && cents !== undefined ? formatCents(cents) : ''}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {footer && <p className="doc-note">{footer}</p>}
    </article>
  );
}

/**
 * Loads an invoice, renders it into the print root, and opens the print dialog once it is on the
 * page. Returns [portal element to render, print(invoiceId)].
 */
export function useInvoicePrinter(onError) {
  const company = useCompany();
  const [detail, setDetail] = useState(null);

  useEffect(() => {
    if (!detail) return undefined;
    // Let React commit the document (and the logo image decode) before the dialog snapshots the page.
    const timer = setTimeout(printDocument, 80);
    return () => clearTimeout(timer);
  }, [detail]);

  const print = useCallback(
    async (invoiceId) => {
      try {
        setDetail(await api.getInvoiceDetail(invoiceId));
      } catch (err) {
        onError(errorText(err));
      }
    },
    [onError],
  );

  const portal = detail ? (
    <PrintPortal>
      <InvoiceDocument detail={detail} company={company} />
    </PrintPortal>
  ) : null;
  return [portal, print];
}
