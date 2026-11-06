import { bringBackMessages, resetMyPreferences, setMyPreferences, useMyPreferences } from '../lib/myPreferences.js';
import { beep } from '../lib/confirmable.js';

function Panel({ id, title, onDefault, children, footer }) {
  return (
    <section className="qb-panel" aria-labelledby={`${id}-title`} id={id}>
      <h2 id={`${id}-title`} className="flex items-center border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
        {title}
        {onDefault && (
          <button type="button" className="ml-auto text-xs font-normal normal-case tracking-normal text-qb-link hover:underline" onClick={onDefault}>
            Default
          </button>
        )}
      </h2>
      {children}
      {footer && <p className="px-2 pb-1 text-2xs text-qb-ink-faint">{footer}</p>}
    </section>
  );
}

function Legend({ children }) {
  return <legend className="px-2 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">{children}</legend>;
}

/** The checkbox and radio controls for My Preferences, bound to `prefs`. */
function useControls() {
  const prefs = useMyPreferences();
  const hinted = (label, hint) => (
    <span>
      {label}
      {hint && <span className="block text-2xs text-qb-ink-faint">{hint}</span>}
    </span>
  );
  const check = (key, label, hint) => (
    <label className="flex items-start gap-1.5 px-2 py-0.5 text-sm">
      <input type="checkbox" className="mt-0.5 accent-qb-green" checked={prefs[key]} onChange={(e) => setMyPreferences({ [key]: e.target.checked })} />
      {hinted(label, hint)}
    </label>
  );
  const radio = (key, value, label, hint) => (
    <label className="flex items-start gap-1.5 px-2 py-0.5 text-sm">
      <input type="radio" name={key} className="mt-0.5 accent-qb-green" checked={prefs[key] === value} onChange={() => setMyPreferences({ [key]: value })} />
      {hinted(label, hint)}
    </label>
  );
  return { prefs, check, radio };
}

/**
 * Preferences › General, the My Preferences part: how typing and the screens behave for you on this
 * computer. Each choice takes effect at once, in every LocalBooks window.
 */
export default function GeneralPreferences() {
  const { prefs, check, radio } = useControls();
  const hidden = prefs.hiddenMessages?.length ?? 0;

  return (
    <Panel id="general-preferences" title="General · My Preferences" onDefault={resetMyPreferences} footer="For you on this computer. In Preferences, Enter always moves to the next box.">
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-2 py-1">
        <div className="flex flex-col gap-1">
          <fieldset>
            <Legend>Typing</Legend>
            {check('enterMovesBetweenFields', 'Pressing Enter moves between fields', 'Enter goes to the next box, as Tab does. Click a button (Save, Post) to record the transaction.')}
            {check('autoOpenDropDowns', 'Open the list of matches as you type', 'Name boxes such as Vendor show matching names, and offer to add a new one, while you type. When off, press ↓ to open the list.')}
            {check('autoDecimalPoint', 'Automatically place a decimal point', 'Amounts typed without a point are in cents: 1234 is 12.34, 5 is 0.05. Typing a point (12.) still means dollars.')}
            {check('tooltipsForClippedText', 'Show the full text of clipped entries on hover', 'Text cut off with “…” in a list shows in full when you point at it.')}
          </fieldset>
          <fieldset>
            <Legend>Date for new transactions</Legend>
            {radio('newTransactionDate', 'today', "Today's date")}
            {radio('newTransactionDate', 'last', 'The last date you entered', 'Handy when entering a stack of older paperwork: each new form starts on the date of the one before.')}
          </fieldset>
          <fieldset>
            <Legend>Sound</Legend>
            <div className="flex items-start">
              {check('beepWhenRecorded', 'Beep when recording a transaction')}
              <button type="button" className="ml-auto mr-2 text-xs text-qb-link hover:underline" onClick={beep}>
                Test
              </button>
            </div>
          </fieldset>
        </div>
        <div className="flex flex-col gap-1">
          <fieldset>
            <Legend>Warnings</Legend>
            {check('warnBeforeEditing', 'Warn when editing a posted transaction', 'Saving changes voids the original and posts the edited one; this asks first.')}
            {check('warnBeforeVoiding', 'Warn when voiding a transaction', 'Void Transaction asks before reversing it.')}
            <div className="flex items-center gap-2 px-2 py-0.5 text-sm">
              <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs" onClick={bringBackMessages} disabled={hidden === 0}>
                Bring back all one-time messages
              </button>
              <span className="text-2xs text-qb-ink-faint">{hidden === 0 ? 'No messages are turned off.' : `${hidden} message${hidden === 1 ? '' : 's'} turned off with “Don’t show this message again”.`}</span>
            </div>
          </fieldset>
          <fieldset>
            <Legend>When the item on a sales line changes</Legend>
            <p className="px-2 text-2xs text-qb-ink-faint">If you typed your own description or price on the line:</p>
            {radio('keepTypedItemDetails', 'ask', 'Ask whether to keep it')}
            {radio('keepTypedItemDetails', 'keep', 'Always keep what I typed')}
            {radio('keepTypedItemDetails', 'replace', "Never: use the new item's description and price")}
          </fieldset>
        </div>
      </div>
    </Panel>
  );
}

/** Preferences › Accounting, the My Preferences part. */
export function AccountingMyPreferences() {
  const { check } = useControls();
  return (
    <Panel id="accounting-my-preferences" title="Accounting · My Preferences" footer="For you on this computer.">
      <div className="py-1">{check('autofillJournalMemo', 'Autofill memo in journal entries', 'A line you start below the first one takes the first line’s memo; change it as you like.')}</div>
    </Panel>
  );
}
