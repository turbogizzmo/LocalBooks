import { Icon } from '../lib/icons.js';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { useSalesPrefs } from '../lib/salesPrefs.js';
import { CELL, TILE, PANELS, SIDE_PANELS, balanceGroups, badgeCounts, flowPath, visiblePanels } from '../lib/homeFlow.js';

const BADGE_TEXT = {
  overdueInvoices: (n) => `${n} overdue`,
  billsDue: (n) => `${n} due soon`,
  toDeposit: (n) => `${n} waiting`,
  todos: (n) => `${n} due`,
};

/** One step: an icon tile with its name under it, and a count when work is waiting. */
function Step({ step, count, onOpen, style, wide }) {
  const waiting = count ? BADGE_TEXT[step.badge](count) : '';
  const name = waiting ? `${step.label}, ${waiting}` : step.label;
  return (
    <button
      type="button"
      onClick={onOpen}
      title={name}
      style={style}
      className={`group flex items-center rounded-sm text-qb-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-qb-green ${
        wide ? 'w-full gap-2 px-1 py-1 text-left hover:bg-qb-row-hover' : 'absolute flex-col gap-1 px-0.5 pt-1.5'
      }`}
    >
      <span
        className="relative grid flex-none place-items-center rounded-md border border-qb-border-strong bg-white text-qb-green-active shadow-xs group-hover:border-qb-green group-hover:bg-qb-green-tint"
        style={{ width: wide ? 28 : TILE, height: wide ? 28 : TILE }}
      >
        <Icon name={step.icon} size={wide ? 16 : 22} />
        {count > 0 && (
          <span aria-hidden="true" className="absolute -right-2 -top-2 min-w-[18px] rounded-full bg-qb-negative px-1 text-center text-2xs font-bold leading-[18px] text-white">
            {count > 99 ? '99+' : count}
          </span>
        )}
      </span>
      {/* The name read out is the visible name, then what is waiting ("Invoices, 5 overdue"). */}
      <span className={`text-xs leading-tight group-hover:underline ${wide ? '' : 'text-center'}`}>
        {step.label}
        {waiting && <span className="sr-only">, {waiting}</span>}
      </span>
    </button>
  );
}

function FlowPanel({ panel, counts, navigate }) {
  const width = panel.cols * CELL.w;
  const height = panel.rows * CELL.h;
  const byId = Object.fromEntries(panel.steps.map((s) => [s.id, s]));
  const markerId = `home-arrow-${panel.id}`;
  return (
    <section className="qb-panel flex flex-col" aria-labelledby={`home-${panel.id}`}>
      <h2 id={`home-${panel.id}`} className="border-b border-qb-border px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
        {panel.title}
      </h2>
      <div className="overflow-x-auto px-2 py-1 scrollbar-thin">
        <div className="relative mx-auto" style={{ width, height }}>
          <svg aria-hidden="true" className="pointer-events-none absolute inset-0 text-qb-border-strong" width={width} height={height}>
            <defs>
              <marker id={markerId} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M0,0 L8,4 L0,8 z" fill="currentColor" />
              </marker>
            </defs>
            {panel.flows.map(([a, b]) => (
              <path key={`${a}-${b}`} d={flowPath(byId[a], byId[b])} fill="none" stroke="currentColor" strokeWidth="2" markerEnd={`url(#${markerId})`} />
            ))}
          </svg>
          {panel.steps.map((s) => (
            <Step
              key={s.id}
              step={s}
              count={s.badge ? counts[s.badge] : 0}
              onOpen={() => navigate(s.view, s.action ?? null)}
              style={{ left: s.col * CELL.w, top: s.row * CELL.h, width: CELL.w, height: CELL.h }}
            />
          ))}
        </div>
      </div>
    </section>
  );
}

function ListPanel({ panel, counts, navigate }) {
  return (
    <section className="qb-panel" aria-labelledby={`home-${panel.id}`}>
      <h2 id={`home-${panel.id}`} className="border-b border-qb-border px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
        {panel.title}
      </h2>
      <div className="grid grid-cols-2 gap-x-1 p-1">
        {panel.steps.map((s) => (
          <Step key={s.id} step={s} wide count={s.badge ? counts[s.badge] : 0} onOpen={() => navigate(s.view, s.action ?? null)} />
        ))}
      </div>
    </section>
  );
}

/** Balances of the bank, card, receivable and payable accounts; each opens its register. */
function Balances({ accounts, navigate }) {
  const groups = balanceGroups(accounts);
  return (
    <section className="qb-panel flex min-h-0 flex-col" aria-labelledby="home-balances">
      <h2 id="home-balances" className="border-b border-qb-border px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
        Balances
      </h2>
      <div className="min-h-0 flex-1 overflow-auto px-2 py-1 text-sm scrollbar-thin">
        {groups.length === 0 && <p className="text-qb-ink-muted">No bank, card, receivable or payable accounts yet.</p>}
        {groups.map((g) => (
          <div key={g.type} className="mb-1">
            <h3 className="text-2xs font-semibold uppercase text-qb-ink-muted">{g.label}</h3>
            <ul>
              {g.accounts.map((a) => (
                <li key={a.id}>
                  <button
                    type="button"
                    className="flex w-full items-baseline gap-2 rounded-xs px-0.5 text-left hover:bg-qb-row-hover"
                    title={`Open the ${a.fullName} register`}
                    onClick={() => navigate('accounting/register', 'open', { accountId: a.id })}
                  >
                    <span className="min-w-0 flex-1 truncate text-qb-link">{a.fullName}</span>
                    <span className={`num flex-none ${a.balanceCents < 0 ? 'num-negative' : ''}`}>{formatCents(a.balanceCents)}</span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
    </section>
  );
}

/**
 * The Home tab: the day-to-day forms as a map of the order the work goes in, shortcuts to banking
 * and company lists, and account balances. Steps for features that are off are left out.
 */
export default function HomeFlow({ accounts, navigate }) {
  const sales = useSalesPrefs();
  const prefs = useLedgerQuery(api.getPreferences).data;
  const reminders = useLedgerQuery(api.getReminders).data;
  const undeposited = useLedgerQuery(api.listUndeposited).data;
  const counts = badgeCounts(reminders, undeposited);
  const panels = visiblePanels(PANELS, { estimates: sales.estimates, inventory: prefs?.inventory_enabled !== 'false' });
  const side = visiblePanels(SIDE_PANELS, {});

  return (
    <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_minmax(240px,320px)] gap-2 overflow-auto scrollbar-thin">
      <div className="flex min-w-0 flex-col gap-2">
        {panels.map((p) => (
          <FlowPanel key={p.id} panel={p} counts={counts} navigate={navigate} />
        ))}
      </div>
      <div className="flex min-w-0 flex-col gap-2">
        {side.map((p) => (
          <ListPanel key={p.id} panel={p} counts={counts} navigate={navigate} />
        ))}
        <Balances accounts={accounts} navigate={navigate} />
      </div>
    </div>
  );
}
