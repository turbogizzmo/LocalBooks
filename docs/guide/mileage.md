# Track vehicle mileage

Record the trips you drive for business, see the deductible amount at the standard mileage rate,
and bill trips to customers. Trips are records: they don't post to your books. A trip you bill
posts through its invoice, like any other invoice.

Open **Employees › Mileage** (or **Employees › Enter Vehicle Mileage** in the menu bar).

## Add your vehicles

1. Click the **Vehicles** tab, then **New vehicle**.
2. Enter a **Vehicle name** (for example `2021 F-150`) and an optional **Description** (year,
   make, model or plate), then click **Save**.

To stop using a vehicle, click **Make inactive**. Its trips stay in the reports, but you can't
record new trips for it. **Make active** brings it back.

## Enter the mileage rate

A trip's deductible amount is its miles × the mileage rate in effect on the trip date. LocalBooks
doesn't come with any rates, because they change: enter the official rate yourself.

1. Find the rate. The IRS publishes its standard mileage rates (business, medical or moving, and
   charitable) at **irs.gov/tax-professionals/standard-mileage-rates**. Check there, or ask your
   accountant, at the start of each year and whenever a mid-year change is announced.
2. Click the **Mileage rates** tab.
3. Enter **Effective from** (the first day the rate applies), the **Rate ($ per mile)**, for
   example `0.70`, and an optional **Note** such as `IRS business rate`. Click **Add rate**.

A rate applies from its date until the next rate's date. When the rate changes mid-year, add the
new rate with its own date: trips before it keep the old rate. Trips dated before your first rate
have no deductible amount, and the Trips list and the reports say how many there are.

## Enter a trip

1. Click the **Trips** tab (or choose **+ New › Mileage Trip**).
2. Choose the **Vehicle** and the **Trip date**.
3. Enter the miles one of two ways:
   - both **Odometer start** and **Odometer end**, and LocalBooks works out the **Total miles**; or
   - just the **Total miles**, for example `12.5`.
4. Optionally choose a **Customer:Job** and enter the **Notes / purpose**.
5. To bill the customer for the trip, tick **Billable**. A billable trip needs a customer or job.
6. Click **Save trip** (or press <kbd>Enter</kbd> in Notes).

The list below shows the trips in the date range, with the rate on each trip's date and the
deductible amount. Click **Edit** or **Delete** to change a trip. A trip that's on an invoice can't
be changed or deleted until that invoice is voided.

## Bill trips to a customer

Set up a mileage item first: in **Sales › Products & Services**, add a service such as `Mileage`
with the income account it should post to and your usual sales rate per mile.

1. Click the **Bill trips** tab. It lists billable trips that aren't on an invoice yet.
2. Choose the **Customer:Job**. Its trips are all ticked; untick any you don't want to bill now.
3. Choose the **Mileage item** (an item with "mile" in its name is chosen for you) and check the
   **Sales rate per mile**. It starts at the item's rate.
4. Check the **Invoice date** and **Due date**, then click **Create invoice**.

LocalBooks creates and posts one invoice with a line for each trip: the miles as the quantity at
your sales rate, described with the trip's date, vehicle and notes. The invoice is listed under
**Sales › Invoices**, where you can print, email or edit it like any other. Editing the invoice
keeps the trips on it. **Voiding** the invoice makes its trips billable again.

The sales rate is what you charge your customer. It's separate from the mileage rate, which is
what the trip is worth as a deduction.

## Mileage reports

Under **Reports › Jobs**:

- **Mileage by Vehicle Summary**: trips, miles and deductible amount for each vehicle.
- **Mileage by Vehicle Detail**: each trip, grouped by vehicle, with its odometer readings, rate
  and deductible amount.
- **Mileage by Job Summary**: trips, miles, billable miles and deductible amount for each customer
  and job. Trips without one are under **No customer/job**.
- **Mileage by Job Detail**: each trip, grouped by customer and job, with whether it's billed.

Choose the **Report period** and a vehicle or customer:job. A customer includes its jobs. Click a
summary row to see its trips. Each trip's deductible amount is rounded to the cent, and the totals
add up those amounts. Export to Excel or CSV, or print, as with any report.
