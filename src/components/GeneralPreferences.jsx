import { resetMyPreferences, setMyPreferences, useMyPreferences } from '../lib/myPreferences.js';

/**
 * Preferences › General, the My Preferences part: how typing and the screens behave for you on this
 * computer. Each choice takes effect at once, in every LocalBooks window.
 */
export default function GeneralPreferences() {
  const prefs = useMyPreferences();
  const check = (key, label, hint) => (
    <label className="flex items-start gap-1.5 px-2 py-0.5 text-sm">
      <input type="checkbox" className="mt-0.5 accent-qb-green" checked={prefs[key]} onChange={(e) => setMyPreferences({ [key]: e.target.checked })} />
      <span>
        {label}
        {hint && <span className="block text-2xs text-qb-ink-faint">{hint}</span>}
      </span>
    </label>
  );
  const radio = (value, label, hint) => (
    <label className="flex items-start gap-1.5 px-2 py-0.5 text-sm">
      <input
        type="radio"
        name="new-transaction-date"
        className="mt-0.5 accent-qb-green"
        checked={prefs.newTransactionDate === value}
        onChange={() => setMyPreferences({ newTransactionDate: value })}
      />
      <span>
        {label}
        {hint && <span className="block text-2xs text-qb-ink-faint">{hint}</span>}
      </span>
    </label>
  );

  return (
    <section className="qb-panel" aria-labelledby="general-prefs-title" id="general-preferences">
      <h2 id="general-prefs-title" className="flex items-center border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
        General · My Preferences
        <button type="button" className="ml-auto text-xs font-normal normal-case tracking-normal text-qb-link hover:underline" onClick={resetMyPreferences}>
          Default
        </button>
      </h2>
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-2 py-1">
        <fieldset>
          <legend className="px-2 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Typing</legend>
          {check('enterMovesBetweenFields', 'Pressing Enter moves between fields', 'Enter goes to the next box, as Tab does. Click a button (Save, Post) to record the transaction.')}
          {check('autoOpenDropDowns', 'Open the list of matches as you type', 'Name boxes such as Vendor show matching names, and offer to add a new one, while you type. When off, press ↓ to open the list.')}
          {check('tooltipsForClippedText', 'Show the full text of clipped entries on hover', 'Text cut off with “…” in a list shows in full when you point at it.')}
        </fieldset>
        <fieldset>
          <legend className="px-2 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Date for new transactions</legend>
          {radio('today', "Today's date")}
          {radio('last', 'The last date you entered', 'Handy when entering a stack of older paperwork: each new form starts on the date of the one before.')}
        </fieldset>
      </div>
      <p className="px-2 pb-1 text-2xs text-qb-ink-faint">For you on this computer. In Preferences, Enter always moves to the next box.</p>
    </section>
  );
}
