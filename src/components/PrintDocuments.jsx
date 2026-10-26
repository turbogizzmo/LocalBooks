import { createContext, useContext } from 'react';
import { createPortal } from 'react-dom';
import { formatMoney } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

/** Company identity for printed documents: name, contact lines and logo (data URL or null). */
export const CompanyContext = createContext({ name: 'LocalBooks', legalName: '', address: '', email: '', phone: '', ein: '', logo: null, homeCurrency: 'USD', fxLookup: false });

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
