import { useCallback, useEffect, useRef, useState } from 'react';
import { useCompany } from './PrintDocuments.jsx';
import { api, errorText } from '../lib/api.js';
import { formatDate } from '../lib/dates.js';
import { formatRate, parseRateE6 } from '../lib/money.js';

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;

/**
 * Suggests the ECB rate for `currency` on `date` when "Look up daily exchange rates" is on.
 *
 * The suggestion fills the rate field only while the field is empty or still holds a default: a
 * rate the hook filled itself, or one passed to `adopt` (such as a payment's invoice rate). A rate
 * the user typed, or one loaded from a saved transaction, is never replaced; the note offers the
 * suggestion with a Use button instead. Returns the note to show beside the field and `adopt`.
 */
export function useSuggestedRate({ currency, date, rate, setRate, initialIsDefault = false }) {
  const { fxLookup } = useCompany();
  const [state, setState] = useState(null);
  // The value the field holds by default (replaceable), or null once the user has chosen a rate.
  const defaultRef = useRef(initialIsDefault ? rate : null);
  const rateRef = useRef(rate);
  rateRef.current = rate;

  const adopt = useCallback(
    (value) => {
      defaultRef.current = value;
      setRate(value);
    },
    [setRate],
  );

  useEffect(() => {
    if (!fxLookup || !currency || !ISO_DATE.test(date ?? '')) {
      setState(null);
      return undefined;
    }
    let stale = false;
    setState({ status: 'loading' });
    const t = setTimeout(() => {
      api.lookupExchangeRate(currency, date).then(
        (s) => {
          if (stale) return;
          setState({ status: 'ok', suggestion: s });
          const current = rateRef.current;
          if (current === '' || current === defaultRef.current) {
            const text = formatRate(s.rateE6);
            defaultRef.current = text;
            setRate(text);
          }
        },
        (err) => !stale && setState({ status: 'error', message: errorText(err) }),
      );
    }, 250);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [fxLookup, currency, date, setRate]);

  let note = null;
  if (state?.status === 'loading') note = <span className="text-qb-ink-faint">Looking up rate…</span>;
  else if (state?.status === 'error') note = <span className="text-qb-warning" title={state.message}>No rate found; enter it yourself</span>;
  else if (state?.status === 'ok') {
    const s = state.suggestion;
    const text = formatRate(s.rateE6);
    const differs = parseRateE6(rate) !== s.rateE6;
    note = (
      <span title={s.stale ? 'The rate service could not be reached; this is the latest rate saved on this computer.' : `${s.source}, published ${s.rateDate}`}>
        {differs ? `ECB ${text}` : 'ECB'} · {formatDate(s.rateDate)}
        {s.stale && ' (offline)'}
        {differs && (
          <button type="button" className="ml-1 text-qb-info underline" onClick={() => adopt(text)}>
            Use
          </button>
        )}
      </span>
    );
  }
  return {
    note: note && (
      <div className="whitespace-nowrap text-2xs leading-tight text-qb-ink-muted" data-testid="rate-note">
        {note}
      </div>
    ),
    error: state?.status === 'error' ? state.message : null,
    adopt,
  };
}
