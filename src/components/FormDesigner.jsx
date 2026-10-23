import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { FormSheet } from './FormPages.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatDate } from '../lib/dates.js';

const KINDS = [
  { id: 'INVOICE', label: 'Invoice' },
  { id: 'ESTIMATE', label: 'Estimate' },
];
const ACCENTS = ['#2ca01c', '#0b6e4f', '#1f4e79', '#2b6cb0', '#6b46c1', '#b83280', '#c05621', '#333333'];
const PLACEHOLDERS = '{number} {customer} {date} {due_date} {total} {balance} {company}';

function Section({ title, children }) {
  return (
    <section className="border-b border-qb-border last:border-b-0">
      <h3 className="bg-qb-row-header px-2 py-0.5 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">{title}</h3>
      <div className="flex flex-col gap-1 px-2 py-1">{children}</div>
    </section>
  );
}

function Row({ label, htmlFor, children }) {
  return (
    <div className="grid grid-cols-[96px_minmax(0,1fr)] items-center gap-2">
      <label htmlFor={htmlFor} className="text-xs font-semibold text-qb-ink-muted">
        {label}
      </label>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function Check({ label, checked, onChange }) {
  return (
    <label className="flex items-center gap-1.5 text-sm">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      {label}
    </label>
  );
}

function Choice({ id, value, options, onChange }) {
  return (
    <select id={id} className="qb-input w-full" value={value} onChange={(e) => onChange(e.target.value)}>
      {options.map(([v, label]) => (
        <option key={v} value={v}>
          {label}
        </option>
      ))}
    </select>
  );
}

/** Documents of the chosen kind the preview can show instead of the sample. */
function useDocumentChoices(kind) {
  const invoices = useLedgerQuery(api.listInvoices);
  const estimates = useLedgerQuery(api.listEstimates);
  const rows =
    kind === 'INVOICE'
      ? (invoices.data ?? []).map((i) => ({ id: i.id, label: `Invoice ${i.invoiceNumber} · ${i.customerName} · ${formatDate(i.issueDate)}` }))
      : (estimates.data ?? []).map((e) => ({ id: e.id, label: `Estimate ${e.estimateNumber} · ${e.customerName} · ${formatDate(e.estimateDate)}` }));
  return rows.slice(0, 50);
}

/**
 * Settings › Custom Forms: designs the printed invoice and estimate. Changes preview live with the
 * sample document or a real one and apply to printing and PDF files once saved.
 */
export default function FormDesigner() {
  const [kind, setKind] = useState('INVOICE');
  const [saved, setSaved] = useState(null);
  const [draft, setDraft] = useState(null);
  const [documentId, setDocumentId] = useState('');
  const [preview, setPreview] = useState(null);
  const [previewError, setPreviewError] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);
  const choices = useDocumentChoices(kind);

  useEffect(() => {
    let cancelled = false;
    setSaved(null);
    setDraft(null);
    setDocumentId('');
    api
      .getFormTemplate(kind)
      .then((t) => {
        if (cancelled) return;
        setSaved(t);
        setDraft(t);
      })
      .catch((err) => !cancelled && setNotice({ kind: 'error', text: errorText(err) }));
    return () => {
      cancelled = true;
    };
  }, [kind]);

  // Re-render the preview shortly after the last change.
  useEffect(() => {
    if (!draft) return undefined;
    let cancelled = false;
    const timer = setTimeout(() => {
      api
        .previewForm(kind, draft, documentId ? Number(documentId) : null)
        .then((form) => {
          if (cancelled) return;
          setPreview(form);
          setPreviewError(null);
        })
        .catch((err) => !cancelled && setPreviewError(errorText(err)));
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [kind, draft, documentId]);

  const dirty = draft && saved && JSON.stringify(draft) !== JSON.stringify(saved);
  const set = (key) => (value) => setDraft((d) => ({ ...d, [key]: value }));
  const text = (key, props = {}) => (
    <input id={`form-${key}`} className="qb-input w-full" value={draft[key]} onChange={(e) => set(key)(e.target.value)} {...props} />
  );

  const save = async () => {
    setBusy(true);
    try {
      const stored = await api.saveFormTemplate(kind, draft);
      setSaved(stored);
      setDraft(stored);
      setNotice({ kind: 'success', text: `${KINDS.find((k) => k.id === kind).label} form saved. Printing and PDF files now use it.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };
  const reset = async () => {
    setBusy(true);
    try {
      const stored = await api.resetFormTemplate(kind);
      setSaved(stored);
      setDraft(stored);
      setNotice({ kind: 'success', text: 'Restored the standard layout.' });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="flex flex-none flex-wrap items-center gap-2">
        <div className="flex gap-px overflow-hidden rounded-xs border border-qb-border-strong text-sm" role="tablist" aria-label="Form">
          {KINDS.map((k) => (
            <button
              key={k.id}
              type="button"
              role="tab"
              aria-selected={kind === k.id}
              onClick={() => {
                if (dirty && !window.confirm('Discard unsaved changes to this form?')) return;
                setKind(k.id);
              }}
              className={`px-3 py-0.5 font-semibold ${kind === k.id ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}
            >
              {k.label}
            </button>
          ))}
        </div>
        <label htmlFor="form-preview-doc" className="text-xs font-semibold text-qb-ink-muted">
          Preview with
        </label>
        <select id="form-preview-doc" className="qb-input w-[320px]" value={documentId} onChange={(e) => setDocumentId(e.target.value)}>
          <option value="">Sample {kind === 'INVOICE' ? 'invoice' : 'estimate'}</option>
          {choices.map((c) => (
            <option key={c.id} value={c.id}>
              {c.label}
            </option>
          ))}
        </select>
        <span className="ml-auto flex gap-1">
          <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy || !dirty} onClick={() => setDraft(saved)}>
            Undo changes
          </button>
          <button
            type="button"
            className="qb-btn qb-btn-secondary h-row"
            disabled={busy || !saved}
            onClick={() => window.confirm('Replace this form with the standard layout?') && reset()}
          >
            Reset to standard
          </button>
          <button type="button" className="qb-btn qb-btn-primary h-row" disabled={busy || !dirty} onClick={save}>
            Save form
          </button>
        </span>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {!draft ? (
        <div className="px-2 py-1 text-sm text-qb-ink-muted">Loading…</div>
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-[340px_minmax(0,1fr)] gap-2">
          <div className="qb-panel min-h-0 overflow-auto scrollbar-thin">
            <Section title="Layout">
              <Row label="Title" htmlFor="form-title">
                {text('title', { maxLength: 40 })}
              </Row>
              <Row label="Paper" htmlFor="form-paper">
                <Choice id="form-paper" value={draft.paper} onChange={set('paper')} options={[['LETTER', 'US Letter (8.5 × 11 in)'], ['A4', 'A4 (210 × 297 mm)']]} />
              </Row>
              <Row label="Font" htmlFor="form-font">
                <Choice id="form-font" value={draft.font} onChange={set('font')} options={[['HELVETICA', 'Helvetica (sans serif)'], ['TIMES', 'Times (serif)'], ['COURIER', 'Courier (typewriter)']]} />
              </Row>
              <Row label="Accent color" htmlFor="form-accent">
                <div className="flex items-center gap-1">
                  <input id="form-accent" type="color" className="h-[22px] w-[34px] cursor-pointer border border-qb-border" value={draft.accentColor} onChange={(e) => set('accentColor')(e.target.value)} />
                  {ACCENTS.map((c) => (
                    <button
                      key={c}
                      type="button"
                      title={c}
                      aria-label={`Accent ${c}`}
                      onClick={() => set('accentColor')(c)}
                      className={`h-[16px] w-[16px] rounded-full border ${draft.accentColor === c ? 'border-qb-ink ring-1 ring-qb-ink' : 'border-qb-border'}`}
                      style={{ background: c }}
                    />
                  ))}
                </div>
              </Row>
              <Row label="Logo" htmlFor="form-logo">
                <div className="flex gap-1">
                  <Choice id="form-logo" value={draft.logo} onChange={set('logo')} options={[['RIGHT', 'Top right'], ['LEFT', 'Top left'], ['HIDDEN', 'Hidden']]} />
                  <Choice id="form-logo-size" value={draft.logoSize} onChange={set('logoSize')} options={[['SMALL', 'Small'], ['MEDIUM', 'Medium'], ['LARGE', 'Large']]} />
                </div>
              </Row>
              <p className="text-2xs text-qb-ink-faint">The logo and company details come from Preferences › Company.</p>
            </Section>
            <Section title="Header details">
              <Check label="Company address" checked={draft.showCompanyAddress} onChange={set('showCompanyAddress')} />
              <Check label="Company phone" checked={draft.showCompanyPhone} onChange={set('showCompanyPhone')} />
              <Check label="Company email" checked={draft.showCompanyEmail} onChange={set('showCompanyEmail')} />
              <Check label="EIN" checked={draft.showEin} onChange={set('showEin')} />
              <Check label="Customer email under the billing address" checked={draft.showCustomerEmail} onChange={set('showCustomerEmail')} />
            </Section>
            <Section title="Line items">
              <Check label="Shaded header row" checked={draft.shadeTableHeader} onChange={set('shadeTableHeader')} />
              <Check label="Line numbers" checked={draft.showLineNumbers} onChange={set('showLineNumbers')} />
              <Check label="Quantity column" checked={draft.showQuantity} onChange={set('showQuantity')} />
              <Check label="Rate column" checked={draft.showRate} onChange={set('showRate')} />
              <Row label="Description" htmlFor="form-descriptionLabel">
                {text('descriptionLabel', { maxLength: 24 })}
              </Row>
              {draft.showQuantity && (
                <Row label="Quantity" htmlFor="form-quantityLabel">
                  {text('quantityLabel', { maxLength: 24 })}
                </Row>
              )}
              {draft.showRate && (
                <Row label="Rate" htmlFor="form-rateLabel">
                  {text('rateLabel', { maxLength: 24 })}
                </Row>
              )}
              <Row label="Amount" htmlFor="form-amountLabel">
                {text('amountLabel', { maxLength: 24 })}
              </Row>
              {kind === 'INVOICE' && <Check label="List payments and credits above the balance due" checked={draft.showPayments} onChange={set('showPayments')} />}
            </Section>
            <Section title="Messages">
              <label htmlFor="form-message" className="text-xs font-semibold text-qb-ink-muted">
                Message to customer
              </label>
              <textarea id="form-message" rows={3} maxLength={600} className="qb-input h-auto w-full py-0.5" value={draft.message} onChange={(e) => set('message')(e.target.value)} />
              <label htmlFor="form-terms" className="text-xs font-semibold text-qb-ink-muted">
                Terms or payment instructions
              </label>
              <textarea
                id="form-terms"
                rows={3}
                maxLength={1000}
                className="qb-input h-auto w-full py-0.5"
                placeholder="e.g. Pay by bank transfer to …, or your terms and conditions"
                value={draft.terms}
                onChange={(e) => set('terms')(e.target.value)}
              />
              <Row label="Page footer" htmlFor="form-footer">
                {text('footer', { maxLength: 150, placeholder: 'e.g. your website' })}
              </Row>
              <p className="text-2xs text-qb-ink-faint">Placeholders: {PLACEHOLDERS}</p>
            </Section>
          </div>
          <div className="qb-panel flex min-h-0 flex-col bg-qb-canvas">
            <div className="flex h-row flex-none items-center gap-2 border-b border-qb-border px-2 text-xs text-qb-ink-muted">
              <span className="font-semibold text-qb-ink">Preview</span>
              {preview && <span>{preview.pages.length === 1 ? '1 page' : `${preview.pages.length} pages`}</span>}
              {dirty && <span className="font-semibold text-qb-warning">Unsaved changes</span>}
            </div>
            {previewError && <Notice kind="error" text={previewError} />}
            <div className="min-h-0 flex-1 overflow-auto p-3 scrollbar-thin">
              <div className="mx-auto flex max-w-[720px] flex-col gap-3">
                {preview?.pages.map((page, i) => (
                  <FormSheet key={i} page={page} logo={preview.logo} className="w-full shadow-md" />
                ))}
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
