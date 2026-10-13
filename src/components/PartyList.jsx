import { useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';

const CONFIG = {
  customer: { list: api.listCustomers, create: api.createCustomer, noun: 'customer', title: 'Customers', balance: 'Open balance (A/R)' },
  vendor: { list: api.listVendors, create: api.createVendor, noun: 'vendor', title: 'Vendors', balance: 'Open balance (A/P)' },
};

/** Customer or vendor list with an inline "add" row at the top of the grid. */
export default function PartyList({ kind }) {
  const cfg = CONFIG[kind];
  const { data, error } = useLedgerQuery(cfg.list);
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  const [query, setQuery] = useState('');

  const add = async (e) => {
    e.preventDefault();
    if (!name.trim()) return setNotice({ kind: 'error', text: `Enter a ${cfg.noun} name.` });
    setBusy(true);
    try {
      const created = await cfg.create(name.trim(), email.trim());
      setNotice({ kind: 'success', text: `Added ${cfg.noun} ${created.name}.` });
      setName('');
      setEmail('');
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const rows = (data ?? []).filter((p) => p.name.toLowerCase().includes(query.trim().toLowerCase()));
  const total = rows.reduce((s, p) => s + p.openBalanceCents, 0);

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">{cfg.title}</h2>
          <input className="qb-input h-row w-[220px] text-sm" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={`Find a ${cfg.noun}`} aria-label="Filter" />
          <span className="ml-auto text-sm text-qb-ink-muted">
            {rows.length} {cfg.noun}s · open <span className="num font-semibold text-qb-ink">{formatCents(total)}</span>
          </span>
        </div>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {error && <Notice kind="error" text={error} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <form onSubmit={add}>
            <table className="qb-grid">
              <colgroup>
                <col />
                <col className="w-[280px]" />
                <col className="w-[160px]" />
                <col className="w-[96px]" />
              </colgroup>
              <thead>
                <tr>
                  <th className="qb-grid-head">Name</th>
                  <th className="qb-grid-head">Email</th>
                  <th className="qb-grid-head text-right">{cfg.balance}</th>
                  <th className="qb-grid-head" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                <tr className="bg-qb-row-selected">
                  <td className="qb-grid-cell-edit">
                    <input value={name} onChange={(e) => setName(e.target.value)} placeholder={`New ${cfg.noun} name`} aria-label="Name" />
                  </td>
                  <td className="qb-grid-cell-edit">
                    <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="name@example.com" aria-label="Email" />
                  </td>
                  <td className="qb-grid-cell" />
                  <td className="qb-grid-cell cell-tight">
                    <button type="submit" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>
                      Add
                    </button>
                  </td>
                </tr>
                {data && rows.length === 0 && (
                  <tr>
                    <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">
                      {query ? `No ${cfg.noun}s match “${query}”.` : `No ${cfg.noun}s yet. They are also created automatically when you post a transaction.`}
                    </td>
                  </tr>
                )}
                {rows.map((p) => (
                  <tr key={p.id} className="qb-grid-row">
                    <td className="qb-grid-cell font-semibold">{p.name}</td>
                    <td className="qb-grid-cell text-qb-ink-muted">{p.email ?? ''}</td>
                    <td className="qb-grid-cell num">{p.openBalanceCents ? formatCents(p.openBalanceCents) : ''}</td>
                    <td className="qb-grid-cell" />
                  </tr>
                ))}
              </tbody>
            </table>
          </form>
        </div>
      </section>
    </div>
  );
}
