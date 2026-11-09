// Pay Bills' Set Discount and Set Credits (T37): the discount taken and the vendor credits used on
// one bill, chosen by hand in place of what Preferences › Bills would take automatically. Pure, so
// `node --test` covers it.

/**
 * The plan to pay with: `plan` ({ discounts: [{ billId, amountCents }], credits: [{ creditId,
 * billId, amountCents }] }, the automatic one) with each bill in `overrides` ({ [billId]:
 * { discountCents?, credits? } }) taking its own discount and credits instead. A discount of 0
 * takes none; a credit of 0 isn't used.
 */
export function withOverrides(plan, overrides) {
  const bills = Object.keys(overrides ?? {}).map(Number);
  const own = (key) => (billId) => overrides[billId]?.[key] !== undefined;
  const discounts = plan.discounts.filter((d) => !own('discountCents')(d.billId));
  const credits = plan.credits.filter((c) => !own('credits')(c.billId));
  for (const billId of bills) {
    const o = overrides[billId];
    if (o.discountCents !== undefined && o.discountCents > 0) discounts.push({ billId, amountCents: o.discountCents });
    if (o.credits !== undefined) {
      for (const c of o.credits) if (c.amountCents > 0) credits.push({ creditId: c.creditId, billId, amountCents: c.amountCents });
    }
  }
  return { discounts, credits };
}

/** How much of `credit` ({ id, remainingCents }) is left for `billId`, after what `credits` uses of it on other bills. */
export function creditLeftFor(credit, credits, billId) {
  const elsewhere = credits.filter((c) => c.creditId === credit.id && c.billId !== billId).reduce((s, c) => s + c.amountCents, 0);
  return Math.max(0, credit.remainingCents - elsewhere);
}

/**
 * Why a bill's own discount and credits can't be taken, or null: each amount at least zero, the
 * discount no more than `offeredCents`, each credit no more than is left of it, and together no more
 * than the bill's balance.
 */
export function overrideProblem({ discountCents, credits }, { balanceCents, offeredCents, leftOf }) {
  if (discountCents !== undefined) {
    if (discountCents === null || discountCents < 0) return 'Enter the discount as an amount of 0.00 or more.';
    if (discountCents > offeredCents) return offeredCents > 0 ? 'That is more than the discount the terms offer on this date.' : 'The bill’s terms offer no discount on this date.';
  }
  for (const c of credits ?? []) {
    if (c.amountCents === null || c.amountCents < 0) return 'Enter each credit as an amount of 0.00 or more.';
    if (c.amountCents > leftOf(c.creditId)) return 'That is more of a credit than is left of it.';
  }
  const total = (discountCents ?? 0) + (credits ?? []).reduce((s, c) => s + (c.amountCents ?? 0), 0);
  if (total > balanceCents) return 'The discount and credits come to more than the bill’s balance.';
  return null;
}
