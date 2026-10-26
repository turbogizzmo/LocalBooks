import { COMMON_CURRENCIES, parseRateE6 } from '../lib/money.js';
import { useSuggestedRate } from './RateSuggestion.jsx';

/**
 * Currency picker plus exchange rate for documents that may be in a foreign currency.
 * `currency` is '' for the home currency. The rate is home units per one foreign unit. With rate
 * lookup on, an empty rate is filled with the ECB rate for `date`; a saved rate is kept.
 */
export default function CurrencyFields({ homeCurrency, currency, setCurrency, rate, setRate, date, disabled = false }) {
  const rateE6 = parseRateE6(rate);
  const { note } = useSuggestedRate({ currency: disabled ? '' : currency, date, rate, setRate });
  const options = [homeCurrency, ...COMMON_CURRENCIES.filter((c) => c !== homeCurrency)];
  return (
    <>
      <label>
        <span className="qb-label">Currency</span>
        <select
          className="qb-input w-[86px]"
          value={currency || homeCurrency}
          disabled={disabled}
          onChange={(e) => setCurrency(e.target.value === homeCurrency ? '' : e.target.value)}
          aria-label="Currency"
        >
          {options.map((code) => (
            <option key={code} value={code}>{code}</option>
          ))}
        </select>
      </label>
      {currency && (
        <label>
          <span className="qb-label" title={`How many ${homeCurrency} one ${currency} is worth`}>
            1 {currency} = ? {homeCurrency}
          </span>
          <span className="flex items-center gap-1.5">
            <input
              className="qb-input num w-[104px]"
              value={rate}
              onChange={(e) => setRate(e.target.value)}
              placeholder="1.000000"
              inputMode="decimal"
              disabled={disabled}
              aria-invalid={rate !== '' && rateE6 === null}
              aria-label="Exchange rate"
            />
            {note}
          </span>
        </label>
      )}
    </>
  );
}
