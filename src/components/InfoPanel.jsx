import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatQuantity } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { infoPanelHidden, partySummaryLines, setInfoPanelHidden } from '../lib/infoPanel.js';
import { useOpenFromList } from '../lib/windows.js';
import { transactionTarget } from '../lib/openTransaction.js';

/** How many recent transactions the panel lists. */
const RECENT = 5;

/**
 * Whether the information panel shows in `place` ('customer', 'vendor', 'item', 'form'), and the
 * button that hides or shows it. The choice is kept on this computer.
 */
export function useInfoPanel(place) {
  const [hidden, setHidden] = useState(() => infoPanelHidden(place));
  const toggle = () => {
    setInfoPanelHidden(place, !hidden);
    setHidden(!hidden);
  };
  const button = (
    <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={toggle} aria-pressed={!hidden} title={hidden ? 'Show the information panel' : 'Hide the information panel'}>
      {hidden ? 'Show info' : 'Hide info'}
    </button>
  );
  return { shown: !hidden, button, hide: () => !hidden && toggle(), show: () => hidden && toggle() };
}

function Shell({ title, children, onHide }) {
  return (
    <aside className="qb-panel flex w-[280px] flex-none flex-col overflow-hidden" aria-label={`${title}: information`}>
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="truncate text-md font-semibold" title={title}>
          {title}
        </h2>
        {onHide && (
          <button type="button" className="ml-auto text-qb-ink-faint hover:text-qb-ink" onClick={onHide} aria-label="Hide the information panel" title="Hide">
            ×
          </button>
        )}
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto p-2 text-sm scrollbar-thin">{children}</div>
    </aside>
  );
}

function Section({ title, children }) {
  return (
    <section>
      <h3 className="mb-0.5 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">{title}</h3>
      {children}
    </section>
  );
}

function Figures({ lines }) {
  return (
    <dl className="grid grid-cols-[1fr_auto] gap-x-2 gap-y-0.5">
      {lines.map(([label, value, note]) => (
        <div key={label} className="contents">
          <dt>
            {label}
            {note && <span className="text-xs text-qb-ink-muted"> · {note}</span>}
          </dt>
          <dd className="num text-right font-semibold">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

/**
 * The information panel for a customer or vendor: its summary (open balance; active estimates and
 * unbilled costs for a customer; open purchase orders for a vendor), phone and email, notes and
 * recent transactions. The figures come from `get_party_summary`, which uses the same functions as
 * the lists and reports. Double-click a recent transaction to open it.
 */
export function PartyInfoPanel({ kind, id, onHide }) {
  const party = useLedgerQuery(() => api.getParty(kind, id));
  const summary = useLedgerQuery(() => api.getPartySummary(kind, id));
  const recent = useLedgerQuery(() =>
    api.findTransactions({ filters: { names: [{ kind, id }], excludeVoided: true, transactionTypes: [] }, startDate: null, endDate: '9999-12-31', limit: RECENT }),
  );
  const { reload: reloadParty } = party;
  const { reload: reloadSummary } = summary;
  const { reload: reloadRecent } = recent;
  useEffect(() => {
    reloadParty();
    reloadSummary();
    reloadRecent();
  }, [kind, id, reloadParty, reloadSummary, reloadRecent]);
  const openFromList = useOpenFromList();
  const p = party.data;
  const error = party.error || summary.error || recent.error;
  const rows = [...(recent.data?.rows ?? [])].sort((a, b) => String(b.date).localeCompare(String(a.date))).slice(0, RECENT);

  return (
    <Shell title={p?.name ?? (kind === 'customer' ? 'Customer' : 'Vendor')} onHide={onHide}>
      {error && <Notice kind="error" text={errorText(error)} />}
      <Section title="Summary">
        {summary.data ? <Figures lines={partySummaryLines(kind, summary.data).map(([label, cents, note]) => [label, cents === null ? '' : formatCents(cents), note])} /> : <span className="text-qb-ink-muted">Loading…</span>}
      </Section>
      {p && (
        <Section title="Contact">
          {p.companyName && <div>{p.companyName}</div>}
          {p.phone ? <div>📞 {p.phone}</div> : <div className="text-qb-ink-faint">No phone</div>}
          {p.email ? (
            <a className="text-qb-link hover:underline" href={`mailto:${p.email}`}>
              ✉ {p.email}
            </a>
          ) : (
            <div className="text-qb-ink-faint">No email</div>
          )}
        </Section>
      )}
      {p?.notes && (
        <Section title="Notes">
          <p className="whitespace-pre-wrap">{p.notes}</p>
        </Section>
      )}
      <Section title="Recent transactions">
        {rows.length === 0 && recent.data && <span className="text-qb-ink-muted">None yet.</span>}
        <ul>
          {rows.map((r) => (
            <li key={r.journalEntryId ?? `${r.typeLabel}-${r.num}-${r.date}`}>
              <button
                type="button"
                className="flex w-full items-baseline gap-1 rounded-sm px-0.5 text-left hover:bg-qb-row-hover"
                onDoubleClick={() => openFromList(...transactionTarget(r))}
                title="Double-click to open it"
              >
                <span className="w-[72px] flex-none text-xs text-qb-ink-muted">{formatDate(r.date)}</span>
                <span className="truncate">
                  {r.typeLabel}
                  {r.num ? ` ${r.num}` : ''}
                </span>
                <span className="num ml-auto">{r.amountCents !== null && r.amountCents !== undefined ? formatCents(r.amountCents) : ''}</span>
              </button>
            </li>
          ))}
        </ul>
      </Section>
    </Shell>
  );
}

/**
 * The information panel for a product or service: price, and for a tracked item on hand, on order
 * (and when the next delivery is due), average cost and value, as Inventory Valuation and Stock
 * Status show them (`get_item_summary`).
 */
export function ItemInfoPanel({ product, onHide }) {
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  useEffect(() => {
    let live = true;
    setData(null);
    setError(null);
    api.getItemSummary(product.id).then(
      (s) => live && setData(s),
      (err) => live && setError(errorText(err)),
    );
    return () => {
      live = false;
    };
  }, [product.id]);
  const lines = data
    ? [
        ['Sales price', formatCents(data.salesPriceCents), null],
        ...(data.tracked
          ? [
              ['On hand', formatQuantity(data.qtyOnHandE4), null],
              ['On order', formatQuantity(data.qtyOnOrderE4), data.nextDelivery ? `next ${formatDate(data.nextDelivery)}` : null],
              ['Average cost', data.averageCostCents === null ? '—' : formatCents(data.averageCostCents), null],
              ['Value', formatCents(data.assetValueCents), null],
            ]
          : []),
      ]
    : [];
  return (
    <Shell title={product.name} onHide={onHide}>
      {error && <Notice kind="error" text={error} />}
      <Section title="Summary">{data ? <Figures lines={lines} /> : !error && <span className="text-qb-ink-muted">Loading…</span>}</Section>
      {data && !data.tracked && <p className="text-xs text-qb-ink-muted">Its stock isn&apos;t tracked. Choose Track inventory on its row to count it.</p>}
      {product.description && (
        <Section title="Description">
          <p className="whitespace-pre-wrap">{product.description}</p>
        </Section>
      )}
    </Shell>
  );
}

/**
 * The information panel on a form, for the customer or vendor chosen on it (`name`, matched in
 * `names`, the list the form already has). Nothing shows until a known name is chosen; once hidden
 * (×), a slim Info button brings it back, on every form, remembered on this computer.
 */
export function FormInfoPanel({ kind, name, names }) {
  const info = useInfoPanel('form');
  const typed = (name ?? '').trim().toLocaleLowerCase();
  const party = typed ? (names ?? []).find((p) => p.name.toLocaleLowerCase() === typed) : null;
  if (!party) return null;
  if (!info.shown) {
    return (
      <button
        type="button"
        className="qb-panel flex w-[22px] flex-none items-center justify-center text-2xs font-semibold text-qb-ink-muted hover:text-qb-ink [writing-mode:vertical-rl]"
        onClick={info.show}
        title={`Show information about ${party.name}`}
      >
        Info
      </button>
    );
  }
  return <PartyInfoPanel kind={kind} id={party.id} onHide={info.hide} />;
}
