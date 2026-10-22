import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';

const SECTIONS = [
  { id: 'ASSET', label: 'Assets' },
  { id: 'LIABILITY', label: 'Liabilities' },
  { id: 'EQUITY', label: 'Equity' },
  { id: 'INCOME', label: 'Income' },
  { id: 'EXPENSE', label: 'Expenses' },
];
// Like classic desktop accounting software, balances are shown for balance sheet accounts only;
// income and expense accounts are read for a period on the Profit and Loss.
const HAS_BALANCE = new Set(['ASSET', 'LIABILITY', 'EQUITY']);

/**
 * Orders accounts as a tree (each parent followed by its subaccounts), keeping the server's
 * chart order (type, then number, then name) among siblings, and rolls balances up to parents.
 */
function buildTree(accounts) {
  const children = new Map();
  for (const a of accounts) {
    const key = a.parentId ?? 0;
    if (!children.has(key)) children.set(key, []);
    children.get(key).push(a);
  }
  const ordered = [];
  const rolled = new Map();
  const visit = (a) => {
    ordered.push(a);
    let total = a.balanceCents;
    for (const child of children.get(a.id) ?? []) total += visit(child);
    rolled.set(a.id, total);
    return total;
  };
  for (const root of children.get(0) ?? []) visit(root);
  return { ordered, rolled, hasChildren: (id) => (children.get(id) ?? []).length > 0 };
}

function AccountDialog({ account, accounts, types, onClose, onSaved }) {
  const editing = Boolean(account);
  const [type, setType] = useState(account?.accountType ?? 'EXPENSE');
  const [name, setName] = useState(account?.name ?? '');
  const [number, setNumber] = useState(account?.accountNumber ?? '');
  const [parentId, setParentId] = useState(account?.parentId ?? null);
  const [description, setDescription] = useState(account?.description ?? '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  // A subaccount shares its parent's type and nests at most five levels; never beneath itself.
  const below = (a) => account && (a.id === account.id || a.fullName.startsWith(`${account.fullName}:`));
  const parents = accounts.filter((a) => a.isActive && a.accountType === type && a.depth < 4 && !below(a));

  const submit = async (e) => {
    e.preventDefault();
    if (!name.trim()) return setError('Enter an account name.');
    setBusy(true);
    const input = { name: name.trim(), accountNumber: number.trim() || null, accountType: type, parentId, description: description.trim() || null };
    try {
      const saved = editing ? await api.updateAccount(account.id, input) : await api.createAccount(input);
      onSaved(`${editing ? 'Saved' : 'Added'} ${saved.accountNumber ? `${saved.accountNumber} · ` : ''}${saved.fullName}.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Dialog title={editing ? `Edit ${account.fullName}` : 'New account'} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        {account?.isSystem && (
          <p className="text-qb-ink-muted">LocalBooks posts to this account automatically, so its type stays fixed and it cannot be a subaccount. You can rename it or change its number.</p>
        )}
        <div className="grid grid-cols-2 gap-2">
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Account type</span>
            <select
              className="qb-input"
              value={type}
              onChange={(e) => {
                setType(e.target.value);
                setParentId(null);
              }}
              disabled={account?.isSystem}
            >
              {SECTIONS.map((section) => (
                <optgroup key={section.id} label={section.label}>
                  {types.filter((t) => t.classification === section.id).map((t) => (
                    <option key={t.code} value={t.code}>{t.displayName}</option>
                  ))}
                </optgroup>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Number (optional)</span>
            <input className="qb-input" value={number} onChange={(e) => setNumber(e.target.value)} placeholder="e.g. 6510" />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Account name</span>
            <input className="qb-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Equipment Rental" />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Subaccount of</span>
            <select className="qb-input" value={parentId ?? ''} onChange={(e) => setParentId(e.target.value === '' ? null : Number(e.target.value))} disabled={account?.isSystem}>
              <option value="">— None (top level) —</option>
              {parents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.accountNumber ? `${a.accountNumber} · ${a.fullName}` : a.fullName}
                </option>
              ))}
            </select>
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Description (optional)</span>
            <input className="qb-input" value={description} onChange={(e) => setDescription(e.target.value)} />
          </label>
        </div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>{editing ? 'Save' : 'Add account'}</button>
        </div>
      </form>
    </Dialog>
  );
}

export default function ChartOfAccounts({ accounts, intent }) {
  const types = useLedgerQuery(api.listAccountTypes).data ?? [];
  const [search, setSearch] = useState('');
  const [showInactive, setShowInactive] = useState(false);
  const [dialog, setDialog] = useState(null); // { account } (null account = new)
  const [notice, setNotice] = useState(null);

  // + New › Account opens the new-account form.
  useEffect(() => {
    if (intent?.action === 'new-account') setDialog({ account: null });
  }, [intent]);

  const { ordered, rolled, hasChildren } = useMemo(() => buildTree(accounts), [accounts]);
  const query = search.trim().toLowerCase();
  const matches = (a) =>
    (showInactive || a.isActive) &&
    (!query || a.fullName.toLowerCase().includes(query) || (a.accountNumber ?? '').includes(query) || a.accountTypeName.toLowerCase().includes(query));
  const inactiveCount = accounts.filter((a) => !a.isActive).length;

  const setActive = async (a, active) => {
    try {
      await api.setAccountActive(a.id, active);
      setNotice({ kind: 'success', text: active ? `${a.fullName} is active again.` : `${a.fullName} is inactive: it is hidden from account pickers, and its history is kept.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {dialog && (
        <AccountDialog
          account={dialog.account}
          accounts={accounts}
          types={types}
          onClose={() => setDialog(null)}
          onSaved={(text) => {
            setDialog(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Chart of Accounts</h2>
          <input className="qb-input h-row w-[220px] text-sm" value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Find by name, number or type" aria-label="Find account" />
          <label className="flex items-center gap-1 text-sm">
            <input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} />
            Show inactive{inactiveCount > 0 ? ` (${inactiveCount})` : ''}
          </label>
          <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setDialog({ account: null })}>
            New account
          </button>
        </div>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[84px]" />
              <col />
              <col className="w-[190px]" />
              <col className="w-[260px]" />
              <col className="w-[140px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">Number</th>
                <th className="qb-grid-head">Account</th>
                <th className="qb-grid-head">Type</th>
                <th className="qb-grid-head">Description</th>
                <th className="qb-grid-head text-right">Balance</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {SECTIONS.map((section) => {
                const rows = ordered.filter((a) => a.classification === section.id && matches(a));
                if (rows.length === 0) return null;
                const total = rows.reduce((sum, a) => sum + a.balanceCents, 0);
                return [
                  <tr key={section.id} className="bg-qb-row-header">
                    <td className="qb-grid-cell font-semibold" colSpan={4}>{section.label}</td>
                    <td className="qb-grid-cell num font-semibold">{HAS_BALANCE.has(section.id) && !query ? formatCents(total) : ''}</td>
                    <td className="qb-grid-cell" />
                  </tr>,
                  ...rows.map((a) => (
                    <tr key={a.id} className={`qb-grid-row ${a.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setDialog({ account: a })}>
                      <td className="qb-grid-cell text-qb-ink-muted">{a.accountNumber ?? ''}</td>
                      <td className="qb-grid-cell" title={a.fullName}>
                        <span style={{ paddingLeft: `${(query ? 0 : a.depth) * 16}px` }}>{query ? a.fullName : a.name}</span>
                        {!a.isActive && <span className="ml-1 text-2xs uppercase tracking-wide">Inactive</span>}
                      </td>
                      <td className="qb-grid-cell text-qb-ink-muted">{a.accountTypeName}</td>
                      <td className="qb-grid-cell text-qb-ink-muted" title={a.description ?? ''}>{a.description ?? ''}</td>
                      <td className="qb-grid-cell num" title={hasChildren(a.id) ? 'Includes its subaccounts' : undefined}>
                        {HAS_BALANCE.has(a.classification) ? formatCents(rolled.get(a.id) ?? a.balanceCents) : ''}
                      </td>
                      <td className="qb-grid-cell cell-tight text-center">
                        <RowMenu
                          label={`Actions for ${a.fullName}`}
                          items={[
                            { label: 'Edit…', onSelect: () => setDialog({ account: a }) },
                            a.isActive
                              ? { label: 'Make inactive', disabled: a.isSystem, hint: a.isSystem ? 'LocalBooks posts to this account automatically' : undefined, onSelect: () => setActive(a, false) }
                              : { label: 'Make active', onSelect: () => setActive(a, true) },
                          ]}
                        />
                      </td>
                    </tr>
                  )),
                ];
              })}
              {ordered.filter(matches).length === 0 && (
                <tr>
                  <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">{query ? `No accounts match “${search.trim()}”.` : 'No accounts yet.'}</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        <div className="flex-none border-t border-qb-border px-2 py-0.5 text-xs text-qb-ink-muted">
          Double-click an account to edit it. Balances are shown for balance sheet accounts; see Reports › Profit and Loss for income and expenses over a period.
        </div>
      </section>
    </div>
  );
}
