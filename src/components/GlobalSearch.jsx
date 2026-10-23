import { useEffect, useRef, useState } from 'react';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { api, errorText } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

const KIND = { CUSTOMER: 'Customer', VENDOR: 'Vendor', PRODUCT: 'Product/service', ACCOUNT: 'Account' };

/** Where each hit opens: (view, action, data) for App's navigate. */
function destination(hit) {
  switch (hit.kind) {
    case 'TRANSACTION':
      return ['accounting/journal', 'open-entry', { entryId: hit.id }];
    case 'CUSTOMER':
      return ['sales/customers'];
    case 'VENDOR':
      return ['expenses/vendors'];
    case 'PRODUCT':
      return ['sales/products'];
    default:
      return ['accounting/register', 'open', { accountId: hit.id }];
  }
}

/** Header search: transactions by number, name, memo or amount, and list entries by name (Ctrl/Cmd+K). */
export default function GlobalSearch({ navigate }) {
  const [query, setQuery] = useState('');
  const [hits, setHits] = useState(null);
  const [error, setError] = useState(null);
  const [active, setActive] = useState(0);
  const [open, setOpen] = useState(false);
  const inputRef = useRef(null);
  const boxRef = useRef(null);

  useEffect(() => {
    const q = query.trim();
    if (q.length < 2) {
      setHits(null);
      return undefined;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      api.searchBooks(q).then(
        (found) => {
          if (cancelled) return;
          setHits(found);
          setActive(0);
          setError(null);
        },
        (err) => !cancelled && setError(errorText(err)),
      );
    }, 180);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query]);

  useEffect(() => {
    const onKey = (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    const onDown = (e) => {
      if (!boxRef.current?.contains(e.target)) setOpen(false);
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onDown);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('mousedown', onDown);
    };
  }, []);

  const choose = (hit) => {
    const [view, action = null, data = null] = destination(hit);
    navigate(view, action, data);
    setOpen(false);
    inputRef.current?.blur();
  };
  const onKeyDown = (e) => {
    if (e.key === 'Escape') {
      setOpen(false);
      inputRef.current?.blur();
    } else if (e.key === 'ArrowDown' && hits?.length) {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, hits.length - 1));
    } else if (e.key === 'ArrowUp' && hits?.length) {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === 'Enter' && hits?.[active]) {
      choose(hits[active]);
    }
  };

  return (
    <div ref={boxRef} className="relative flex-none">
      <input
        ref={inputRef}
        type="search"
        className="qb-input h-row w-[260px] text-sm"
        placeholder="Search transactions, names, amounts (⌘K)"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
        aria-label="Search the books"
        role="combobox"
        aria-expanded={open && Boolean(hits)}
        aria-controls="global-search-results"
      />
      {open && (hits || error) && (
        <ul id="global-search-results" role="listbox" className="qb-panel absolute right-0 top-full z-50 mt-1 max-h-[420px] w-[460px] overflow-y-auto py-1 shadow-panel scrollbar-thin">
          {error && <li className="px-2 py-1 text-sm text-qb-negative">{error}</li>}
          {hits?.length === 0 && <li className="px-2 py-1 text-sm text-qb-ink-muted">Nothing matches “{query.trim()}”.</li>}
          {hits?.map((h, i) => (
            <li
              key={`${h.kind}-${h.id}`}
              role="option"
              aria-selected={i === active}
              className={`flex cursor-pointer items-baseline gap-2 px-2 py-0.5 text-sm ${i === active ? 'bg-qb-row-selected' : 'hover:bg-qb-row-hover'}`}
              onMouseEnter={() => setActive(i)}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(h);
              }}
            >
              <span className="w-[96px] flex-none text-xs text-qb-ink-muted">{h.kind === 'TRANSACTION' ? (SOURCE_LABEL[h.sourceType] ?? h.sourceType) : KIND[h.kind]}</span>
              <span className="min-w-0 flex-1 truncate">
                <span className="font-semibold">{h.label}</span>
                {h.detail && h.detail !== h.label && <span className="text-qb-ink-muted"> · {h.detail}</span>}
              </span>
              {h.date && <span className="flex-none text-xs text-qb-ink-muted">{formatDate(h.date)}</span>}
              {h.amountCents !== null && h.amountCents !== undefined && <span className="num w-[90px] flex-none text-right">{formatCents(h.amountCents)}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
