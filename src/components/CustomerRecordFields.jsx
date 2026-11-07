import { centsToInput, parseMoneyToCents } from '../lib/money.js';

/**
 * The fuller customer record in the customer dialog: billing and shipping addresses, contacts and
 * more phone numbers, and the account details (account no., credit limit, resale number). The
 * dialog owns `form`; these only read and set their own keys.
 */

const label = 'font-semibold text-qb-ink-muted';

function Text({ form, setForm, name, title, ...props }) {
  return (
    <label className="flex flex-col gap-0.5">
      <span className={label}>{title}</span>
      <input className="qb-input" value={form[name] ?? ''} onChange={(e) => setForm((f) => ({ ...f, [name]: e.target.value }))} {...props} />
    </label>
  );
}

/** The two contacts, alternate phone, fax and website. */
export function CustomerContactFields({ form, setForm }) {
  return (
    <>
      <Text form={form} setForm={setForm} name="contactName" title="Contact" maxLength={100} />
      <Text form={form} setForm={setForm} name="altContactName" title="Alt. contact" maxLength={100} />
      <Text form={form} setForm={setForm} name="altPhone" title="Alt. phone" type="tel" maxLength={40} />
      <Text form={form} setForm={setForm} name="fax" title="Fax" type="tel" maxLength={40} />
      <div className="col-span-2">
        <Text form={form} setForm={setForm} name="website" title="Website" placeholder="example.com" maxLength={200} />
      </div>
    </>
  );
}

/** Billing and shipping addresses side by side, with a button to copy billing to shipping. */
export function CustomerAddressFields({ form, setForm }) {
  const area = (name) => (
    <textarea className="qb-input h-[70px] py-1" value={form[name] ?? ''} maxLength={500} onChange={(e) => setForm((f) => ({ ...f, [name]: e.target.value }))} />
  );
  return (
    <>
      <label className="flex flex-col gap-0.5">
        <span className={label}>Billing address (Bill To)</span>
        {area('address')}
      </label>
      <label className="flex flex-col gap-0.5">
        <span className="flex items-center justify-between">
          <span className={label}>Shipping address (Ship To)</span>
          <button
            type="button"
            className="text-2xs text-qb-link hover:underline disabled:opacity-40"
            disabled={!(form.address ?? '').trim()}
            title="Use the billing address as the shipping address"
            onClick={() => setForm((f) => ({ ...f, shippingAddress: f.address }))}
          >
            Copy billing
          </button>
        </span>
        {area('shippingAddress')}
      </label>
      {form.parentName && (
        <p className="col-span-2 -mt-1 text-2xs text-qb-ink-faint">Left blank, a job uses {form.parentName}'s addresses on its forms.</p>
      )}
    </>
  );
}

/** Account no., credit limit and resale number. `form.creditLimit` is the text in the box. */
export function CustomerAccountFields({ form, setForm }) {
  const limit = form.creditLimit ?? '';
  const bad = limit.trim() !== '' && (parseMoneyToCents(limit) == null || parseMoneyToCents(limit) < 0);
  return (
    <>
      <Text form={form} setForm={setForm} name="accountNumber" title="Account no." maxLength={50} />
      <label className="flex flex-col gap-0.5">
        <span className={label}>Credit limit</span>
        <input
          className={`qb-input text-right ${bad ? 'border-qb-negative' : ''}`}
          inputMode="decimal" data-money
          placeholder="No limit"
          value={limit}
          title="An invoice that would take the customer over this asks before saving"
          onChange={(e) => setForm((f) => ({ ...f, creditLimit: e.target.value }))}
        />
      </label>
      <Text form={form} setForm={setForm} name="resaleNumber" title="Resale no. / tax ID" maxLength={50} />
    </>
  );
}

/** The dialog's form state for a loaded customer. */
export function customerRecordForm(party) {
  const name = party.name ?? '';
  const parentName = name.includes(':') ? name.slice(0, name.lastIndexOf(':')) : null;
  return { creditLimit: party.creditLimitCents == null ? '' : centsToInput(party.creditLimitCents), parentName };
}

/** The new customer fields for `updateParty`; throws a readable message for a bad credit limit. */
export function customerRecordPayload(form) {
  const text = (form.creditLimit ?? '').trim();
  const creditLimitCents = text === '' ? null : parseMoneyToCents(text);
  if (creditLimitCents == null && text !== '') throw new Error('Enter the credit limit as an amount, or leave it blank for no limit.');
  if (creditLimitCents != null && creditLimitCents < 0) throw new Error('The credit limit cannot be negative.');
  return {
    shippingAddress: form.shippingAddress,
    contactName: form.contactName,
    altContactName: form.altContactName,
    altPhone: form.altPhone,
    fax: form.fax,
    website: form.website,
    accountNumber: form.accountNumber,
    creditLimitCents,
    resaleNumber: form.resaleNumber,
  };
}
