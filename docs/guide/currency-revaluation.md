# Revalue foreign-currency invoices

When customers owe you in another currency, the amount in your home currency changes with exchange
rates. A **revaluation** restates those open invoices at a period-end rate, so the Balance Sheet and
Profit and Loss for that period show what they're worth then.

## Revalue at period end

1. Go to **Accounting › Currency Revaluation**.
2. Set **Revalue as of** to the period end, for example the last day of the month or quarter.
3. Enter the rate for each currency listed (**1 EUR = ? USD**, for example). Use the rate your bank
   or accountant uses for period end. With [daily exchange rates](#look-up-daily-exchange-rates)
   turned on, each field is filled with the ECB rate for the **Revalue as of** date, which you can
   change.
4. Check the table: each open invoice shows what it's carried at, its value at the new rate, and
   the gain or loss.
5. Click **Post revaluation** and confirm.

LocalBooks posts the net gain or loss to **Unrealized Exchange Gain or Loss** on that date and
reverses it the next day. Your books keep each invoice at its original rate, so when the customer
pays, the realized gain or loss (in **Exchange Gain or Loss**) is still exactly right.

## Undo a revaluation

Under **Past revaluations**, click **Void** on the row. Both the revaluation and its reversal are
voided, and you can revalue that date again.

> **Tip:** Only invoices can be in a foreign currency today, so there are no foreign payables to
> revalue. Revalue at most once per date.

## Look up daily exchange rates

LocalBooks can suggest exchange rates for you. It's **off** until you turn it on.

1. Go to **Settings › Preferences Console**.
2. Under **Accounting & tax**, check **Look up daily exchange rates**.

From then on, when you pick a foreign currency on an invoice, expense or check, receive a payment
on a foreign-currency invoice, or revalue, the rate field is filled with the
**European Central Bank's reference rate** for that transaction's date. A note under the field
shows where the rate came from: **ECB** and the date it was published.

- **Weekends and holidays.** The ECB publishes on business days, so a Saturday invoice uses
  Friday's rate. The note shows the date of the rate that was used.
- **You can always change it.** Type a different rate and LocalBooks keeps yours. If you've typed
  a rate, the note shows the ECB rate with a **Use** button instead of replacing it.
- **Saved transactions keep their rate.** Opening a saved invoice or expense never changes its rate,
  and nothing is re-rated when new rates come in.
- **Payments** start from the payment date's rate instead of the invoice's rate, so the realized
  gain or loss reflects the day you were paid.
- **Revaluation** offers the rate for the **Revalue as of** date (the period end).

### What is sent over the internet

Each lookup asks the [Frankfurter](https://github.com/lineofflight/frankfurter) service
(`api.frankfurter.dev`), an open-source service that republishes the ECB's rates, for one rate.
It sends **only the two currency codes and the date**, for example "EUR to USD on 2024-01-12".
Nothing about your company, customers or amounts is sent.

Rates are saved in your company file, so each date is looked up only once, and rates you've already
looked up still appear when you're offline.

### When there's no rate

If the service can't be reached (you're offline, or it doesn't answer within 10 seconds), the note
says **No rate found; enter it yourself** and you type the rate as usual. If a rate from the
previous week was already looked up, that rate is offered instead, marked **(offline)**.

The ECB publishes rates for about 30 major currencies (AUD, BRL, CAD, CHF, CNY, CZK, DKK, EUR, GBP,
HKD, HUF, IDR, ILS, INR, ISK, JPY, KRW, MXN, MYR, NOK, NZD, PHP, PLN, RON, SEK, SGD, THB, TRY, USD
and ZAR), with history back to January 4, 1999. For any other currency or an earlier date, enter
the rate yourself.

> **Tip:** Like every rate in LocalBooks, a suggested rate is rounded to six decimal places. For a
> currency worth much less than your home currency (1 JPY = 0.006874 USD, for example), that leaves
> fewer significant digits than the ECB publishes.
