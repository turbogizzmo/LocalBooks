import { useEffect, useState } from 'react';
import { api, errorText, printDocument, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { todayIso } from '../lib/dates.js';
import DateBox from './DateBox.jsx';
import Notice from './Notice.jsx';
import { PrintPortal } from './PrintDocuments.jsx';

const CLASSIFICATIONS = [
  ['INDIVIDUAL', 'Individual or sole proprietor'],
  ['C_CORPORATION', 'C corporation'],
  ['S_CORPORATION', 'S corporation'],
  ['PARTNERSHIP', 'Partnership'],
  ['TRUST_ESTATE', 'Trust or estate'],
  ['LLC_C', 'LLC taxed as a C corporation'],
  ['LLC_S', 'LLC taxed as an S corporation'],
  ['LLC_P', 'LLC taxed as a partnership'],
  ['OTHER', 'Other'],
];

/**
 * A vendor's Form W-9 (`form_1099.rs`): legal and business name, tax classification, the TIN and
 * its type, and the date the form came in. The TIN is shown masked until opened, and locked for
 * users who may not see sensitive data.
 */
export function W9Dialog({ vendorId, vendorName, onClose }) {
  const [form, setForm] = useState(null);
  const [showTin, setShowTin] = useState(false);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api.getVendorW9(vendorId).then((w) => setForm({ ...w, tinDraft: w.tin ?? '' }), (err) => setError(errorText(err)));
  }, [vendorId]);
  const set = (k) => (e) => setForm((f) => ({ ...f, [k]: e.target.value }));
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const keepTin = form.tinLocked || form.tinDraft === (form.tin ?? '');
      await api.setVendorW9(vendorId, {
        tinType: form.tinType || null,
        tin: keepTin ? null : form.tinDraft,
        keepTin,
        legalName: form.legalName || null,
        businessName: form.businessName || null,
        taxClassification: form.taxClassification || null,
        exemptPayeeCode: form.exemptPayeeCode || null,
        receivedOn: form.receivedOn || null,
      });
      onClose(true);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" role="dialog" aria-modal="true" aria-label={`W-9 for ${vendorName}`}>
      <div className="qb-panel w-[520px] p-3 text-sm">
        <h2 className="mb-2 text-md font-semibold">W-9 · {vendorName}</h2>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        {form && (
          <div className="grid grid-cols-2 gap-2">
            <label className="col-span-2 flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">Name (line 1, as on the tax return)</span>
              <input className="qb-input" value={form.legalName ?? ''} maxLength={150} onChange={set('legalName')} placeholder={vendorName} />
            </label>
            <label className="col-span-2 flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">Business name (line 2), if different</span>
              <input className="qb-input" value={form.businessName ?? ''} maxLength={150} onChange={set('businessName')} />
            </label>
            <label className="col-span-2 flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">Federal tax classification</span>
              <select className="qb-input" value={form.taxClassification ?? ''} onChange={set('taxClassification')}>
                <option value="">Not given</option>
                {CLASSIFICATIONS.map(([code, label]) => (
                  <option key={code} value={code}>{label}</option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">TIN type</span>
              <select className="qb-input" value={form.tinType ?? ''} onChange={set('tinType')} disabled={form.tinLocked}>
                <option value="">Choose…</option>
                <option value="SSN">SSN or ITIN</option>
                <option value="EIN">EIN</option>
              </select>
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">TIN</span>
              {form.tinLocked || (!showTin && form.tinMasked) ? (
                <span className="flex gap-1">
                  <input className="qb-input min-w-0 flex-1 bg-qb-canvas" value={form.tinMasked ?? ''} placeholder="None" readOnly />
                  {!form.tinLocked && (
                    <button type="button" className="qb-btn qb-btn-secondary flex-none px-2" onClick={() => setShowTin(true)}>Show</button>
                  )}
                </span>
              ) : (
                <input className="qb-input" value={form.tinDraft} onChange={set('tinDraft')} autoComplete="off" spellCheck={false} placeholder={form.tinType === 'EIN' ? '12-3456789' : '123-45-6789'} />
              )}
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">Exempt payee code (if any)</span>
              <input className="qb-input" value={form.exemptPayeeCode ?? ''} maxLength={2} onChange={set('exemptPayeeCode')} />
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">W-9 received</span>
              <DateBox className="qb-input" value={form.receivedOn ?? ''} onChange={set('receivedOn')} />
            </label>
            {form.tinLocked && <p className="col-span-2 text-2xs text-qb-ink-faint">Only users who can see sensitive data can view or change the TIN.</p>}
          </div>
        )}
        <div className="mt-3 flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-primary h-row" disabled={!form || busy} onClick={save}>Save</button>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => onClose(false)}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

const payerLines = (p) => [p.name, p.address, p.phone].filter(Boolean).join('\n');

/** One recipient copy (Copy B) of a 1099-NEC, on plain paper, with the TIN truncated. */
function CopyB({ year, payer, form }) {
  return (
    <article className="doc doc-page-break">
      <div className="doc-title">Form 1099-NEC · {year} · Copy B, for the recipient</div>
      <div className="doc-subtitle">Nonemployee Compensation. This is important tax information and is being furnished to the IRS.</div>
      <table className="doc-table" style={{ marginTop: '12pt' }}>
        <tbody>
          <tr><th style={{ width: '40%' }}>Payer’s name, address and phone</th><td style={{ whiteSpace: 'pre-line' }}>{payerLines(payer)}</td></tr>
          <tr><th>Payer’s TIN</th><td>{payer.ein ?? ''}</td></tr>
          <tr><th>Recipient’s TIN</th><td>{form.tinMasked ?? ''}</td></tr>
          <tr><th>Recipient’s name</th><td>{form.recipientName}{form.businessName ? `\n${form.businessName}` : ''}</td></tr>
          <tr><th>Recipient’s address</th><td style={{ whiteSpace: 'pre-line' }}>{form.address ?? ''}</td></tr>
          <tr><th>Box 1 · Nonemployee compensation</th><td className="doc-num">{formatCents(form.box1Cents)}</td></tr>
          <tr><th>Box 4 · Federal income tax withheld</th><td className="doc-num">{formatCents(0)}</td></tr>
        </tbody>
      </table>
      <p style={{ marginTop: '12pt', fontSize: '8pt' }}>
        Report this income on your tax return as the IRS’s instructions for recipients of Form 1099-NEC describe. Your TIN is shown with only its last four digits, as the IRS
        allows on recipient copies.
      </p>
    </article>
  );
}

/**
 * Reports › 1099-NEC and 1096: the year's forms over the threshold, what each still needs (TIN,
 * W-9, address), the 1096 totals, and Print forms (a filer's worksheet with the 1096 totals, then
 * Copy B for each recipient).
 */
export function Forms1099Nec() {
  const thisYear = Number(todayIso().slice(0, 4));
  const [year, setYear] = useState(thisYear - 1);
  const [w9, setW9] = useState(null);
  const { data, error, reload } = useLedgerQuery(() => api.get1099NecSummary(year));
  useEffect(() => {
    reload();
  }, [year, reload]);
  const ready = data && data.forms.length > 0 && data.payerMissing.length === 0 && data.forms.every((f) => f.missing.length === 0);
  return (
    <section className="qb-panel flex min-h-0 flex-1 flex-col">
      <div className="no-print flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="self-center text-md font-semibold">1099-NEC and 1096</h2>
        <label>
          <span className="qb-label">Calendar year</span>
          <select className="qb-input w-[100px]" value={year} onChange={(e) => setYear(Number(e.target.value))}>
            {Array.from({ length: 6 }, (_, i) => thisYear - i).map((y) => (
              <option key={y} value={y}>{y}</option>
            ))}
          </select>
        </label>
        <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" disabled={!data?.forms.length} onClick={() => printDocument(`1099-NEC ${year}`)}>
          Print forms…
        </button>
      </div>
      {error && <Notice kind="error" text={error} />}
      {data && (
        <div className="min-h-0 flex-1 overflow-auto p-2 text-sm scrollbar-thin">
          <p>
            <span className="font-semibold">{data.formCount}</span> form{data.formCount === 1 ? '' : 's'}, <span className="font-semibold">{formatCents(data.totalCents)}</span> in box 1
            (Form 1096: 1099-NEC). Vendors paid {formatCents(data.thresholdCents)} or more in {year} get a form.
          </p>
          {data.payerMissing.length > 0 && <Notice kind="error" text={`Your company is missing: ${data.payerMissing.join(', ')}. Add it in Settings › Company.`} />}
          {data.forms.length > 0 && !ready && <Notice kind="info" text="Some forms are missing details. Click Edit W-9 to fill them in, or add the address on the vendor." />}
          <table className="qb-grid mt-2">
            <thead>
              <tr>
                <th className="qb-grid-head">Recipient</th>
                <th className="qb-grid-head w-[130px]">TIN</th>
                <th className="qb-grid-head w-[130px] text-right">Box 1</th>
                <th className="qb-grid-head">Missing</th>
                <th className="qb-grid-head w-[90px]" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data.forms.map((f) => (
                <tr key={f.vendorId} className="qb-grid-row">
                  <td className="qb-grid-cell">{f.recipientName}{f.businessName ? ` · ${f.businessName}` : ''}</td>
                  <td className="qb-grid-cell font-mono text-xs">{f.tinMasked ?? '—'}</td>
                  <td className="qb-grid-cell num">{formatCents(f.box1Cents)}</td>
                  <td className={`qb-grid-cell ${f.missing.length ? 'text-qb-negative' : 'text-qb-ink-muted'}`}>{f.missing.length ? f.missing.join(', ') : 'Ready'}</td>
                  <td className="qb-grid-cell cell-tight">
                    <button type="button" className="qb-btn qb-btn-secondary h-[20px] w-full" onClick={() => setW9({ id: f.vendorId, name: f.recipientName })}>Edit W-9</button>
                  </td>
                </tr>
              ))}
              {data.forms.length === 0 && (
                <tr><td className="qb-grid-cell text-qb-ink-muted" colSpan={5}>No vendor marked for 1099 tracking was paid {formatCents(data.thresholdCents)} or more in box 1 in {year}.</td></tr>
              )}
            </tbody>
          </table>
          {data.underThreshold.length > 0 && (
            <p className="mt-2 text-xs text-qb-ink-muted">
              Under the threshold, so no form: {data.underThreshold.map(([name, cents]) => `${name} (${formatCents(cents)})`).join(', ')}.
            </p>
          )}
          <p className="mt-2 text-xs text-qb-ink-muted">
            Print forms prints a filer’s worksheet with the Form 1096 totals and every recipient’s full TIN{data.fullTins ? '' : ' (masked: you may not see full TINs)'}, then Copy B for
            each recipient on plain paper. To file, enter the worksheet in the IRS’s free IRIS Taxpayer Portal, or copy it onto the official red-ink Copy A and Form 1096 if you
            file on paper. Send each recipient their Copy B by January 31.
          </p>
        </div>
      )}
      {data && data.forms.length > 0 && (
        <PrintPortal>
          <article className="doc">
            <div className="doc-title">Form 1096 summary · 1099-NEC · {year}</div>
            <div className="doc-subtitle">Filer’s worksheet. Keep it safe: it shows recipients’ full TINs.</div>
            <table className="doc-table" style={{ marginTop: '12pt' }}>
              <tbody>
                <tr><th style={{ width: '40%' }}>Filer’s name and address</th><td style={{ whiteSpace: 'pre-line' }}>{payerLines(data.payer)}</td></tr>
                <tr><th>Employer identification number</th><td>{data.payer.ein ?? ''}</td></tr>
                <tr><th>Contact email</th><td>{data.payer.email ?? ''}</td></tr>
                <tr><th>Total number of forms</th><td className="doc-num">{data.formCount}</td></tr>
                <tr><th>Federal income tax withheld</th><td className="doc-num">{formatCents(0)}</td></tr>
                <tr><th>Total amount reported</th><td className="doc-num">{formatCents(data.totalCents)}</td></tr>
              </tbody>
            </table>
            <table className="doc-table" style={{ marginTop: '12pt' }}>
              <thead>
                <tr><th>Recipient</th><th>TIN type</th><th>TIN</th><th>Address</th><th className="doc-num">Box 1</th></tr>
              </thead>
              <tbody>
                {data.forms.map((f) => (
                  <tr key={f.vendorId}>
                    <td>{f.recipientName}{f.businessName ? ` / ${f.businessName}` : ''}</td>
                    <td>{f.tinType ?? ''}</td>
                    <td>{f.tin ?? f.tinMasked ?? ''}</td>
                    <td style={{ whiteSpace: 'pre-line' }}>{f.address ?? ''}</td>
                    <td className="doc-num">{formatCents(f.box1Cents)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </article>
          {data.forms.map((f) => (
            <CopyB key={f.vendorId} year={year} payer={data.payer} form={f} />
          ))}
        </PrintPortal>
      )}
      {w9 && <W9Dialog vendorId={w9.id} vendorName={w9.name} onClose={(saved) => (setW9(null), saved && reload())} />}
    </section>
  );
}
