import { useMemo, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, parseMoneyToCents } from '../lib/money.js';

const incomeOnly = (a) => a.classification === 'INCOME';

export default function Products({ accounts }) {
  const { data, error } = useLedgerQuery(api.listProducts);
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && incomeOnly(a))?.id ?? null, [accounts]);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [rate, setRate] = useState('');
  const [accountId, setAccountId] = useState(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);

  const chosenAccount = accountId ?? defaultIncome;
  const rateCents = rate.trim() === '' ? 0 : parseMoneyToCents(rate);

  const add = async (e) => {
    e.preventDefault();
    if (!name.trim()) return setNotice({ kind: 'error', text: 'Enter a product or service name.' });
    if (rateCents === null || rateCents < 0) return setNotice({ kind: 'error', text: 'Enter a valid rate.' });
    if (chosenAccount === null) return setNotice({ kind: 'error', text: 'Choose an income account.' });
    setBusy(true);
    try {
      const product = await api.createProduct({
        name: name.trim(),
        description: description.trim() || null,
        rateCents,
        incomeAccountId: chosenAccount,
      });
      setNotice({ kind: 'success', text: `Added ${product.name}.` });
      setName('');
      setDescription('');
      setRate('');
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Products & Services</h2>
          <span className="ml-2 text-xs text-qb-ink-muted">Picking one on an invoice or sales receipt fills its description, rate and income account</span>
        </div>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {error && <Notice kind="error" text={error} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <form onSubmit={add}>
            <table className="qb-grid">
              <colgroup>
                <col className="w-[220px]" />
                <col />
                <col className="w-[112px]" />
                <col className="w-[240px]" />
                <col className="w-[96px]" />
              </colgroup>
              <thead>
                <tr>
                  <th className="qb-grid-head">Name</th>
                  <th className="qb-grid-head">Description</th>
                  <th className="qb-grid-head text-right">Rate</th>
                  <th className="qb-grid-head">Income account</th>
                  <th className="qb-grid-head" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                <tr className="bg-qb-row-selected">
                  <td className="qb-grid-cell-edit">
                    <input value={name} onChange={(e) => setName(e.target.value)} placeholder="New product or service" aria-label="Name" />
                  </td>
                  <td className="qb-grid-cell-edit">
                    <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Description on sales forms" aria-label="Description" />
                  </td>
                  <td className="qb-grid-cell-edit">
                    <input className="num" value={rate} onChange={(e) => setRate(e.target.value)} placeholder="0.00" inputMode="decimal" aria-label="Rate" aria-invalid={rateCents === null} />
                  </td>
                  <td className="qb-grid-cell-edit">
                    <AccountSelect className="" accounts={accounts} filter={incomeOnly} value={chosenAccount} onChange={setAccountId} aria-label="Income account" />
                  </td>
                  <td className="qb-grid-cell cell-tight">
                    <button type="submit" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>
                      Add
                    </button>
                  </td>
                </tr>
                {data && data.length === 0 && (
                  <tr>
                    <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">No products or services yet.</td>
                  </tr>
                )}
                {(data ?? []).map((p) => (
                  <tr key={p.id} className="qb-grid-row">
                    <td className="qb-grid-cell font-semibold">{p.name}</td>
                    <td className="qb-grid-cell text-qb-ink-muted" title={p.description ?? ''}>{p.description ?? ''}</td>
                    <td className="qb-grid-cell num">{p.rateCents ? formatCents(p.rateCents) : ''}</td>
                    <td className="qb-grid-cell">{accountLabel(accounts, p.incomeAccountId)}</td>
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
