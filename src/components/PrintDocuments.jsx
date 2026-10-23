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
export function DocHeader({ company, title, subtitle }) {
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
  // Amount paid includes credit memos applied, which are not payments.
  const paid = invoice.amountPaidCents ?? payments.reduce((s, p) => s + p.amountCents, 0);
  const credits = invoice.creditsAppliedCents ?? 0;
  const tax = invoice.taxCents ?? 0;
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
          {tax > 0 && (
            <>
              <tr>
                <td>Subtotal</td>
                <td className="doc-num">{formatCents(invoice.totalCents - tax)}</td>
              </tr>
              <tr>
                <td>{invoice.taxRateName ? `Sales tax (${invoice.taxRateName})` : 'Sales tax'}</td>
                <td className="doc-num">{formatCents(tax)}</td>
              </tr>
            </>
          )}
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
          {credits > 0 && (
            <tr>
              <td>Credits applied</td>
              <td className="doc-num">-{formatCents(credits)}</td>
            </tr>
          )}
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
  // Columns are labels or { label, kind }; a `percent` column holds basis points.
  const heads = (columns ?? ['Total']).map((c) => (typeof c === 'string' ? c : c.label));
  const kinds = (columns ?? []).map((c) => (typeof c === 'string' ? null : c.kind));
  const format = (c, cents) => (kinds[c] === 'percent' ? `${(cents / 100).toFixed(1)}%` : formatCents(cents));
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
                <td key={c} className="doc-num">{cents !== null && cents !== undefined ? format(c, cents) : ''}</td>
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

const ONES = ['', 'One', 'Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight', 'Nine', 'Ten', 'Eleven', 'Twelve', 'Thirteen', 'Fourteen', 'Fifteen', 'Sixteen', 'Seventeen', 'Eighteen', 'Nineteen'];
const TENS = ['', '', 'Twenty', 'Thirty', 'Forty', 'Fifty', 'Sixty', 'Seventy', 'Eighty', 'Ninety'];

function under1000(n) {
  const words = [];
  if (n >= 100) {
    words.push(`${ONES[Math.floor(n / 100)]} Hundred`);
    n %= 100;
  }
  if (n >= 20) {
    words.push(TENS[Math.floor(n / 10)] + (n % 10 ? `-${ONES[n % 10]}` : ''));
  } else if (n > 0) {
    words.push(ONES[n]);
  }
  return words.join(' ');
}

/** 1234.56 → "One Thousand Two Hundred Thirty-Four and 56/100", as written on a check. */
export function amountInWords(cents) {
  let dollars = Math.floor(Math.abs(cents) / 100);
  const rest = String(Math.abs(cents) % 100).padStart(2, '0');
  if (dollars === 0) return `Zero and ${rest}/100`;
  const parts = [];
  for (const scale of ['', ' Thousand', ' Million', ' Billion']) {
    const chunk = dollars % 1000;
    if (chunk) parts.unshift(`${under1000(chunk)}${scale}`);
    dollars = Math.floor(dollars / 1000);
    if (!dollars) break;
  }
  return `${parts.join(' ')} and ${rest}/100`;
}

/** A voucher check: the check on top, then two stubs with the payee, amount and memo. */
export function CheckDocument({ company, check, bankName }) {
  const amount = formatMoney(check.totalCents, check.currencyCode);
  const stub = (
    <div className="check-stub">
      <div className="check-stub-row">
        <span>{check.payeeName ?? ''}</span>
        <span>{formatDate(check.expenseDate)}</span>
      </div>
      <div className="check-stub-row">
        <span>{check.memo ?? ''}</span>
        <span className="doc-num">{amount}</span>
      </div>
      <div className="check-stub-row check-muted">
        <span>{bankName}</span>
        <span>{check.checkNumber ? `Check #${check.checkNumber}` : ''}</span>
      </div>
    </div>
  );
  return (
    <article className="doc check">
      <section className="check-face">
        <div className="check-row">
          <div>
            <div className="doc-company-name">{company.name}</div>
            <div className="doc-company-lines">{company.address}</div>
          </div>
          <div className="check-number">{check.checkNumber ?? ''}</div>
        </div>
        <div className="check-row check-date">
          <span />
          <span>Date {formatDate(check.expenseDate)}</span>
        </div>
        <div className="check-row check-payee">
          <span>
            Pay to the order of <strong>{check.payeeName ?? ''}</strong>
          </span>
          <span className="check-amount">**{amount}</span>
        </div>
        <div className="check-words">
          {amountInWords(check.totalCents)}
          {' '}
          {'*'.repeat(12)} Dollars
        </div>
        <div className="check-row check-bottom">
          <span>Memo {check.memo ?? ''}</span>
          <span className="check-signature">Authorized signature</span>
        </div>
      </section>
      {stub}
      {stub}
    </article>
  );
}
