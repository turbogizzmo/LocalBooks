import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { FormSheet } from './FormPages.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatDate } from '../lib/dates.js';

const KINDS = [
  { id: 'INVOICE', label: 'Invoice' },
  { id: 'ESTIMATE', label: 'Estimate' },
  { id: 'SALES_RECEIPT', label: 'Sales receipt' },
  { id: 'STATEMENT', label: 'Statement' },
];
const LAYOUTS = [
  ['CLASSIC', 'Classic', 'Company and title side by side over an accent rule'],
  ['MODERN', 'Modern', 'The title in a colored band, shaded table header, banded rows'],
  ['COMPACT', 'Compact', 'Smaller type and tighter rows: more lines per page'],
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
  const sales = useLedgerQuery(api.listAllSales);
  const customers = useLedgerQuery(api.listCustomers);
  const rows = {
    INVOICE: () => (invoices.data ?? []).map((i) => ({ id: i.id, label: `Invoice ${i.invoiceNumber} · ${i.customerName} · ${formatDate(i.issueDate)}` })),
    ESTIMATE: () => (estimates.data ?? []).map((e) => ({ id: e.id, label: `Estimate ${e.estimateNumber} · ${e.customerName} · ${formatDate(e.estimateDate)}` })),
    SALES_RECEIPT: () =>
      (sales.data ?? []).filter((r) => r.kind === 'SALES_RECEIPT').map((r) => ({ id: r.id, label: `Sales receipt ${r.number} · ${r.customerName} · ${formatDate(r.date)}` })),
    // A statement previews last month's statement for the customer.
    STATEMENT: () => (customers.data ?? []).map((c) => ({ id: c.id, label: `${c.name} · last month` })),
  }[kind]();
  return rows.slice(0, 50);
}

/** The style being designed: pick, add, copy, rename, make default or delete. */
function StyleBar({ styles, styleId, setStyleId, dirty, onChanged, onNotice }) {
  const style = styles.find((s) => s.id === styleId);
  const run = async (fn, text) => {
    try {
      const result = await fn();
      await onChanged(result?.id);
      if (text) onNotice({ kind: 'success', text });
    } catch (err) {
      onNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const create = (copy) => {
    const name = window.prompt(copy ? `Name for a copy of “${style.name}”` : 'Name for the new style', copy ? `${style.name} copy` : '');
    if (name) run(() => api.createFormStyle(name, copy ? style.id : null), `Added the style “${name}”.`);
  };
  const rename = () => {
    const name = window.prompt('New name for this style', style.name);
    if (name && name !== style.name) run(() => api.renameFormStyle(style.id, name), `Renamed to “${name}”.`);
  };
  const remove = () =>
    window.confirm(`Delete the style “${style.name}”?${style.customerCount ? ` Its ${style.customerCount} customer(s) go back to the default style.` : ''}`) &&
    run(() => api.deleteFormStyle(style.id), `Deleted “${style.name}”.`);
  return (
    <div className="flex flex-wrap items-center gap-1">
      <label htmlFor="form-style" className="text-xs font-semibold text-qb-ink-muted">
        Style
      </label>
      <select
        id="form-style"
        className="qb-input w-[220px]"
        value={styleId ?? ''}
        onChange={(e) => {
          if (dirty && !window.confirm('Discard unsaved changes to this form?')) return;
          setStyleId(Number(e.target.value));
        }}
      >
        {styles.map((s) => (
          <option key={s.id} value={s.id}>
            {s.name}
            {s.isDefault ? ' (default)' : ''}
            {s.customerCount ? ` · ${s.customerCount} customer${s.customerCount === 1 ? '' : 's'}` : ''}
          </option>
        ))}
      </select>
      <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => create(false)}>
        New
      </button>
      <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => create(true)} disabled={!style}>
        Copy
      </button>
      <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={rename} disabled={!style}>
        Rename
      </button>
      {style && !style.isDefault && (
        <>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => run(() => api.setDefaultFormStyle(style.id), `“${style.name}” is now the default style.`)}>
            Make default
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={remove}>
            Delete
          </button>
        </>
      )}
    </div>
  );
}

/**
 * Settings › Custom Forms: designs the printed invoice, estimate, sales receipt and statement for
 * each form style. Changes preview live with the sample document or a real one and apply to
 * printing, PDF files and email once saved. Customers use the default style unless their
 * customer record picks another.
 */
export default function FormDesigner() {
  const [kind, setKind] = useState('INVOICE');
  const styles = useLedgerQuery(api.listFormStyles);
  const [styleId, setStyleId] = useState(null);
  const [saved, setSaved] = useState(null);
  const [draft, setDraft] = useState(null);
  const [documentId, setDocumentId] = useState('');
  const [preview, setPreview] = useState(null);
  const [previewError, setPreviewError] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);
  const choices = useDocumentChoices(kind);

  const styleList = styles.data ?? [];
  // Start on the default style; after a delete, fall back to it.
  useEffect(() => {
    if (styleList.length && !styleList.some((s) => s.id === styleId)) setStyleId(styleList[0].id);
  }, [styleList, styleId]);

  useEffect(() => {
    if (styleId === null) return undefined;
    let cancelled = false;
    setSaved(null);
    setDraft(null);
    setDocumentId('');
    api
      .getFormTemplate(kind, styleId)
      .then((t) => {
        if (cancelled) return;
        setSaved(t);
        setDraft(t);
      })
      .catch((err) => !cancelled && setNotice({ kind: 'error', text: errorText(err) }));
    return () => {
      cancelled = true;
    };
  }, [kind, styleId]);

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
      const stored = await api.saveFormTemplate(kind, draft, styleId);
      setSaved(stored);
      setDraft(stored);
      setNotice({ kind: 'success', text: `${KINDS.find((k) => k.id === kind).label} form saved in “${styleList.find((s) => s.id === styleId)?.name}”. Printing, PDF files and email now use it.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };
  const reset = async () => {
    setBusy(true);
    try {
      const stored = await api.resetFormTemplate(kind, styleId);
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
      <StyleBar
        styles={styleList}
        styleId={styleId}
        setStyleId={setStyleId}
        dirty={dirty}
        onNotice={setNotice}
        onChanged={async (id) => {
          await styles.reload();
          if (id) setStyleId(id);
        }}
      />
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
          <option value="">Sample {KINDS.find((k) => k.id === kind).label.toLowerCase()}</option>
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
              <div className="grid grid-cols-3 gap-1" role="radiogroup" aria-label="Layout preset">
                {LAYOUTS.map(([id, label, hint]) => (
                  <button
                    key={id}
                    type="button"
                    role="radio"
                    aria-checked={draft.layout === id}
                    title={hint}
                    onClick={() => set('layout')(id)}
                    className={`rounded-xs border px-2 py-1 text-sm ${draft.layout === id ? 'border-qb-nav bg-qb-row-hover font-semibold' : 'border-qb-border bg-white hover:bg-qb-row-hover'}`}
                  >
                    {label}
                  </button>
                ))}
              </div>
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
            {kind === 'STATEMENT' && (
              <Section title="Statement">
                <Check label="Shaded header row" checked={draft.shadeTableHeader || draft.layout === 'MODERN'} onChange={set('shadeTableHeader')} />
                <Check label="Aging summary (current, 1–30, 31–60, 61–90, over 90 days)" checked={draft.showAging} onChange={set('showAging')} />
                <Row label="Amount" htmlFor="form-amountLabel">
                  {text('amountLabel', { maxLength: 24 })}
                </Row>
              </Section>
            )}
            {kind !== 'STATEMENT' && (
              <Section title="Line items">
                <Check label="Shaded header row" checked={draft.shadeTableHeader || draft.layout === 'MODERN'} onChange={set('shadeTableHeader')} />
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
            )}
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
            {(kind === 'INVOICE' || kind === 'ESTIMATE') && (
              <Section title="Email">
                <Row label="Subject" htmlFor="form-emailSubject">
                  {text('emailSubject', { maxLength: 200 })}
                </Row>
                <label htmlFor="form-emailBody" className="text-xs font-semibold text-qb-ink-muted">
                  Message
                </label>
                <textarea id="form-emailBody" rows={6} maxLength={4000} className="qb-input h-auto w-full py-0.5" value={draft.emailBody} onChange={(e) => set('emailBody')(e.target.value)} />
                <p className="text-2xs text-qb-ink-faint">Used by Email… on a row, with the PDF attached. You can edit it before sending.</p>
              </Section>
            )}
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
