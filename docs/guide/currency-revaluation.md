# Revalue foreign-currency invoices

When customers owe you in another currency, the amount in your home currency changes with exchange
rates. A **revaluation** restates those open invoices at a period-end rate, so the Balance Sheet and
Profit and Loss for that period show what they're worth then.

## Revalue at period end

1. Go to **Accounting › Currency Revaluation**.
2. Set **Revalue as of** to the period end, for example the last day of the month or quarter.
3. Enter the rate for each currency listed (**1 EUR = ? USD**, for example). Use the rate your bank
   or accountant uses for period end.
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
