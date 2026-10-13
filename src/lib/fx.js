import { mulDivRound, toHomeCents } from './money.js';

/**
 * Home-currency effect of a payment on a foreign-currency invoice, computed exactly as the ledger
 * posts it: cash received at the payment rate, A/R relieved cumulatively at the invoice's booked
 * rate (the final payment clears A/R exactly), and the realized exchange gain (+) or loss (−).
 */
export function foreignPaymentEffect(invoice, amountCents, rateE6) {
  if (!invoice.currencyCode || amountCents === null || rateE6 === null) return null;
  const homeReceived = toHomeCents(amountCents, rateE6);
  const paidAfter = invoice.amountPaidCents + amountCents;
  const relievedBefore = invoice.homeTotalCents - invoice.homeBalanceCents;
  const relievedAfter = paidAfter === invoice.totalCents ? invoice.homeTotalCents : mulDivRound(invoice.homeTotalCents, paidAfter, invoice.totalCents);
  if (homeReceived === null || relievedAfter === null) return null;
  const arRelief = relievedAfter - relievedBefore;
  return { homeReceived, arRelief, gain: homeReceived - arRelief };
}
