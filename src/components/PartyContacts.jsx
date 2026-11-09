import { useEffect, useState } from 'react';
import { api, errorText } from '../lib/api.js';

/**
 * The contacts on a customer or vendor, in the record dialog: any number, each with a role. A
 * customer's contacts can be the default for invoices (named in Bill To and sent the emailed forms)
 * and for statements; a vendor's can be their main contact.
 */

const ROLES = ['Owner', 'Main', 'Billing', 'Accounts payable', 'Accounts receivable', 'Purchasing', 'Sales', 'Shipping', 'Site', 'Alternate'];
const blank = () => ({ key: Math.random(), id: null, name: '', role: '', email: '', phone: '', mobile: '', forInvoices: false, forStatements: false });

/** Loads a record's contacts; `contacts` is null until they arrive. */
export function usePartyContacts(kind, id) {
  const [contacts, setContacts] = useState(null);
  const [error, setError] = useState(null);
  useEffect(() => {
    let live = true;
    api.listPartyContacts(kind, id).then(
      (rows) => live && setContacts(rows.map((c) => ({ ...c, key: c.id, role: c.role ?? '', email: c.email ?? '', phone: c.phone ?? '', mobile: c.mobile ?? '' }))),
      (err) => live && setError(errorText(err)),
    );
    return () => {
      live = false;
    };
  }, [kind, id]);
  return { contacts, setContacts, error };
}

/** What `savePartyContacts` takes: blank rows (no name and nothing else) are dropped. */
export function contactsPayload(contacts) {
  return contacts
    .filter((c) => [c.name, c.role, c.email, c.phone, c.mobile].some((v) => (v ?? '').trim() !== ''))
    .map(({ id, name, role, email, phone, mobile, forInvoices, forStatements }) => ({ id, name, role, email, phone, mobile, forInvoices, forStatements }));
}

export default function PartyContacts({ kind, contacts, setContacts }) {
  const customer = kind === 'customer';
  const edit = (key, change) => setContacts((list) => list.map((c) => (c.key === key ? { ...c, ...change } : c)));
  // A default is one per use: choosing it on one contact takes it off the others.
  const choose = (key, flag) => setContacts((list) => list.map((c) => ({ ...c, [flag]: c.key === key ? !c[flag] : false })));
  const move = (index, by) =>
    setContacts((list) => {
      const next = [...list];
      const [row] = next.splice(index, 1);
      next.splice(index + by, 0, row);
      return next;
    });
  const cell = (c, name, props = {}) => (
    <td className="qb-grid-cell-edit">
      <input value={c[name] ?? ''} onChange={(e) => edit(c.key, { [name]: e.target.value })} aria-label={`${props.title ?? name} of contact ${c.name || 'new'}`} {...props} />
    </td>
  );
  return (
    <fieldset className="col-span-2 flex flex-col gap-1" aria-label="Contacts">
      <legend className="font-semibold text-qb-ink-muted">
        Contacts{customer ? <span className="font-normal"> · Inv. = the invoice contact, Stmt. = the statement contact</span> : <span className="font-normal"> · Main = the main contact</span>}
      </legend>
      <datalist id="contact-roles">
        {ROLES.map((r) => (
          <option key={r} value={r} />
        ))}
      </datalist>
      <table className="qb-grid table-fixed">
        <colgroup>
          <col className="w-[24%]" />
          <col className="w-[17%]" />
          <col className="w-[27%]" />
          <col className="w-[16%]" />
          <col className="w-[16%]" />
          <col className="w-[34px]" />
          {customer && <col className="w-[34px]" />}
          <col className="w-[50px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">Name</th>
            <th className="qb-grid-head">Role</th>
            <th className="qb-grid-head">Email</th>
            <th className="qb-grid-head">Phone</th>
            <th className="qb-grid-head">Mobile</th>
            <th className="qb-grid-head px-0 text-center" title={customer ? 'Invoices: named in Bill To, and sent emailed invoices, estimates, sales receipts and credit memos' : 'The main contact'}>
              {customer ? 'Inv.' : 'Main'}
            </th>
            {customer && (
              <th className="qb-grid-head px-0 text-center" title="Statements: printed on statements and sent emailed statements">
                Stmt.
              </th>
            )}
            <th className="qb-grid-head" aria-label="Order and remove" />
          </tr>
        </thead>
        <tbody>
          {contacts.length === 0 && (
            <tr>
              <td colSpan={customer ? 8 : 7} className="qb-grid-cell text-qb-ink-muted">
                No contacts yet.
              </td>
            </tr>
          )}
          {contacts.map((c, i) => (
            <tr key={c.key} className="qb-grid-row">
              {cell(c, 'name', { maxLength: 100, title: 'Name' })}
              {cell(c, 'role', { maxLength: 50, list: 'contact-roles', title: 'Role' })}
              {cell(c, 'email', { maxLength: 254, type: 'email', title: 'Email' })}
              {cell(c, 'phone', { maxLength: 40, type: 'tel', title: 'Phone' })}
              {cell(c, 'mobile', { maxLength: 40, type: 'tel', title: 'Mobile' })}
              <td className="qb-grid-cell text-center">
                <input type="checkbox" className="accent-qb-green" checked={c.forInvoices} onChange={() => choose(c.key, 'forInvoices')} aria-label={`${customer ? 'Invoice' : 'Main'} contact: ${c.name || 'new'}`} />
              </td>
              {customer && (
                <td className="qb-grid-cell text-center">
                  <input type="checkbox" className="accent-qb-green" checked={c.forStatements} onChange={() => choose(c.key, 'forStatements')} aria-label={`Statement contact: ${c.name || 'new'}`} />
                </td>
              )}
              <td className="qb-grid-cell whitespace-nowrap px-0 text-center">
                <button type="button" className="px-0.5 text-qb-link disabled:text-qb-ink-faint" disabled={i === 0} onClick={() => move(i, -1)} aria-label={`Move ${c.name || 'contact'} up`}>
                  ↑
                </button>
                <button type="button" className="px-0.5 text-qb-link disabled:text-qb-ink-faint" disabled={i === contacts.length - 1} onClick={() => move(i, 1)} aria-label={`Move ${c.name || 'contact'} down`}>
                  ↓
                </button>
                <button type="button" className="px-0.5 text-qb-negative" onClick={() => setContacts((list) => list.filter((x) => x.key !== c.key))} aria-label={`Remove ${c.name || 'contact'}`}>
                  ×
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div>
        <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={() => setContacts((list) => [...list, blank()])}>
          Add contact
        </button>
      </div>
    </fieldset>
  );
}
