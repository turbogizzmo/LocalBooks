/**
 * The tax rate a new sales form starts with for a customer: none when the customer is tax exempt;
 * the customer's own rate; otherwise the company's default rate (Sales › Sales Tax › Default rate)
 * when it is still active; otherwise the company's only active rate, if it has just one.
 * `customer` is null for a name not on the list yet.
 */
export function startingTaxRateId(customer, activeRates, companyDefaultId) {
  if (customer?.isTaxExempt) return null;
  if (customer?.taxRateId != null) return customer.taxRateId;
  const active = activeRates ?? [];
  if (companyDefaultId != null && active.some((r) => r.id === companyDefaultId)) return companyDefaultId;
  return active.length === 1 ? active[0].id : null;
}

/** The company default rate id stored as a preference ('' or an id), or null. */
export function companyDefaultRateId(prefs) {
  const text = prefs?.sales_tax_default_rate_id ?? '';
  const id = Number(text);
  return text !== '' && Number.isInteger(id) && id > 0 ? id : null;
}
