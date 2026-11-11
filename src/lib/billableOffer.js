// The invoice form's offer of a customer's unbilled costs and time (Preferences › Time & Expenses).

/** Time without a service item can't be billed (its line needs the item's income account). */
export const timeBillable = (t) => t.productServiceId !== null;

/**
 * What the offer names: "2 unbilled costs and 1 unbilled time entry", counting only what the
 * picker lets you tick. Null when there is nothing to offer.
 */
export function billableOfferText(costs, time) {
  const billableTime = time.filter(timeBillable).length;
  const parts = [
    costs.length ? `${costs.length} unbilled cost${costs.length === 1 ? '' : 's'}` : null,
    billableTime ? `${billableTime} unbilled time entr${billableTime === 1 ? 'y' : 'ies'}` : null,
  ].filter(Boolean);
  return parts.length ? parts.join(' and ') : null;
}
