import { useMemo, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';

export const MATCH_KINDS = [
  { value: 'BEGINS', label: 'Begins with' },
  { value: 'CONTAINS', label: 'Contains' },
  { value: 'ENDS', label: 'Ends with' },
];

/** The text a renaming rule starts from: the description's first words, as the bank writes them. */
export function suggestedPattern(description) {
  const words = description.trim().split(/\s+/).filter(Boolean);
  const lead = [];
  for (const w of words) {
    if (!/[A-Za-z]/.test(w) || lead.length === 2) break;
    lead.push(w);
  }
  return lead.length ? lead.join(' ') : (words[0] ?? '');
}

/** One renaming rule's fields: begins with / contains / ends with, the text, and the name. */
export function RuleFields({ rule, onChange, autoFocus = false }) {
  const set = (key) => (e) => onChange({ ...rule, [key]: e.target.value });
  return (
    <>
      <select className="qb-input w-[120px]" value={rule.matchKind} onChange={set('matchKind')} aria-label="When the description">
        {MATCH_KINDS.map((k) => (
          <option key={k.value} value={k.value}>
            {k.label}
          </option>
        ))}
      </select>
      <input className="qb-input w-[180px]" value={rule.pattern} onChange={set('pattern')} aria-label="Text" autoFocus={autoFocus} />
      <span className="self-center text-sm text-qb-ink-muted">use the name</span>
      <input className="qb-input w-[180px]" value={rule.payeeName} onChange={set('payeeName')} aria-label="Name" />
    </>
  );
}

/**
 * Bank feed rules: renaming rules for downloaded payees (search, edit, remove, add) and the
 * category rules learned from approvals (change the account, remove).
 */
export default function BankRules({ accounts, onClose }) {
  const [tab, setTab] = useState('rename');
  const [query, setQuery] = useState('');
  const renames = useLedgerQuery(api.listBankPayeeRules);
  const categories = useLedgerQuery(api.listBankCategoryRules);
  const [editing, setEditing] = useState(null); // a rule being edited or added
  const [notice, setNotice] = useState(null);
  const needle = query.trim().toLowerCase();
  // A new search drops an edit in progress, so its Save/Cancel row never vanishes behind the filter.
  const search = (text) => {
    setQuery(text);
    setEditing(null);
  };
  const kindLabel = (k) => MATCH_KINDS.find((m) => m.value === k)?.label ?? k;
  const shownRenames = useMemo(
    () => (renames.data ?? []).filter((r) => !needle || r.pattern.toLowerCase().includes(needle) || r.payeeName.toLowerCase().includes(needle)),
    [renames.data, needle],
  );
  const shownCategories = useMemo(
    () => (categories.data ?? []).filter((r) => !needle || r.words.toLowerCase().includes(needle) || (accountLabel(accounts, r.accountId) ?? '').toLowerCase().includes(needle)),
    [categories.data, needle, accounts],
  );
  const act = async (work, text) => {
    try {
      await work();
      setNotice(text ? { kind: 'success', text } : null);
      return true;
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
      return false;
    }
  };
  const saveEditing = async () => {
    if (await act(() => api.saveBankPayeeRule(editing), 'Rule saved.')) setEditing(null);
  };

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-qb-nav/40 backdrop-blur-[2px]">
      <div role="dialog" aria-modal="true" aria-labelledby="bank-rules-title" className="flex max-h-[80vh] w-[760px] flex-col rounded-xs bg-white shadow-popover">
        <div className="flex items-center gap-2 border-b border-qb-border px-3 py-1.5">
          <h2 id="bank-rules-title" className="text-md font-semibold">Bank feed rules</h2>
          <div className="ml-3 flex gap-1" role="tablist">
            {[
              ['rename', 'Renaming rules'],
              ['category', 'Category rules'],
            ].map(([id, label]) => (
              <button key={id} type="button" role="tab" aria-selected={tab === id} className={`qb-btn h-row ${tab === id ? 'qb-btn-primary' : 'qb-btn-secondary'}`} onClick={() => setTab(id)}>
                {label}
              </button>
            ))}
          </div>
          <input className="qb-input ml-auto w-[200px]" placeholder="Search" value={query} onChange={(e) => search(e.target.value)} aria-label="Search rules" />
        </div>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {(renames.error || categories.error) && <Notice kind="error" text={renames.error ?? categories.error} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          {tab === 'rename' ? (
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head">When the description</th>
                  <th className="qb-grid-head">Text</th>
                  <th className="qb-grid-head">Use the name</th>
                  <th className="qb-grid-head" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {shownRenames.length === 0 && (
                  <tr>
                    <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">
                      No renaming rules{needle ? ' match' : ' yet'}. Change a payee in the bank feed and Tillhouse offers one.
                    </td>
                  </tr>
                )}
                {shownRenames.map((r) =>
                  editing?.id === r.id ? (
                    <tr key={r.id}>
                      <td colSpan={4} className="qb-grid-cell">
                        <div className="flex flex-wrap gap-1">
                          <RuleFields rule={editing} onChange={setEditing} autoFocus />
                          <button type="button" className="qb-btn qb-btn-primary h-row" onClick={saveEditing}>
                            Save
                          </button>
                          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setEditing(null)}>
                            Cancel
                          </button>
                        </div>
                      </td>
                    </tr>
                  ) : (
                    <tr key={r.id} className="qb-grid-row">
                      <td className="qb-grid-cell">{kindLabel(r.matchKind)}</td>
                      <td className="qb-grid-cell font-mono text-xs">{r.pattern}</td>
                      <td className="qb-grid-cell font-semibold">{r.payeeName}</td>
                      <td className="qb-grid-cell whitespace-nowrap text-right">
                        <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setEditing({ ...r })}>
                          Edit
                        </button>{' '}
                        <button type="button" className="qb-btn qb-btn-secondary h-row text-qb-negative" onClick={() => act(() => api.removeBankPayeeRule(r.id), 'Rule removed.')}>
                          Remove
                        </button>
                      </td>
                    </tr>
                  ),
                )}
                {editing?.id === 0 && (
                  <tr>
                    <td colSpan={4} className="qb-grid-cell">
                      <div className="flex flex-wrap gap-1">
                        <RuleFields rule={editing} onChange={setEditing} autoFocus />
                        <button type="button" className="qb-btn qb-btn-primary h-row" onClick={saveEditing}>
                          Add
                        </button>
                        <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setEditing(null)}>
                          Cancel
                        </button>
                      </div>
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          ) : (
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head">Description has</th>
                  <th className="qb-grid-head">For</th>
                  <th className="qb-grid-head">Category</th>
                  <th className="qb-grid-head text-right">Used</th>
                  <th className="qb-grid-head" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {shownCategories.map((r) => (
                  <tr key={r.id} className="qb-grid-row">
                    <td className="qb-grid-cell">
                      {r.words}
                      {r.origin === 'BUILT_IN' && <span className="ml-1 text-2xs text-qb-ink-muted">built in</span>}
                    </td>
                    <td className="qb-grid-cell">{r.direction === 'OUTFLOW' ? 'Money out' : r.direction === 'INFLOW' ? 'Money in' : 'Either'}</td>
                    <td className="qb-grid-cell-edit">
                      <AccountSelect
                        className=""
                        accounts={accounts}
                        filter={(a) => !['BANK', 'CREDIT_CARD'].includes(a.accountType)}
                        value={r.accountId}
                        onChange={(id) => act(() => api.setBankCategoryRuleAccount(r.id, id), 'Rule changed.')}
                        aria-label={`Category for ${r.words}`}
                      />
                    </td>
                    <td className="qb-grid-cell num">{r.hitCount}</td>
                    <td className="qb-grid-cell text-right">
                      <button type="button" className="qb-btn qb-btn-secondary h-row text-qb-negative" onClick={() => act(() => api.removeBankCategoryRule(r.id), 'Rule removed.')}>
                        Remove
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-qb-border px-3 py-1.5">
          {tab === 'rename' && (
            <button type="button" className="qb-btn qb-btn-secondary mr-auto" onClick={() => setEditing({ id: 0, matchKind: 'BEGINS', pattern: '', payeeName: '' })} disabled={editing !== null}>
              Add rule
            </button>
          )}
          <button type="button" className="qb-btn qb-btn-primary" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
