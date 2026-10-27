// Help, part 1: topics on Sales & customers (the number in the file name sets the order) and the help for its screens.
// Each topic’s own `area` decides its heading under All topics.

/** What each screen is for (`help`) and its main guide (`guide`, docs/guide/<id>.md), shown at the top of Help. */
export const screens = {
  'sales/all': {
    help: 'Every sale in one register: invoices, payments and sales receipts. Record a sales receipt here when a customer pays at the time of sale.',
    guide: 'sales-receipts-and-estimates',
  },
  'sales/invoices': {
    help: 'Create invoices, see what each customer still owes, and record full or partial payments against one or several invoices.',
    guide: 'invoices',
  },
  'sales/estimates': {
    help: 'Quotes for customers that do not touch your books until they become invoices. Mark an estimate accepted, then convert it to an invoice in one step.',
    guide: 'sales-receipts-and-estimates',
  },
  'sales/tax': {
    help: 'Turn on sales tax, set up the rates you charge, see taxable sales and tax owed for any period, and record payments to the tax agency.',
  },
  'sales/credits': {
    help: 'Credit memos give a customer credit to apply to their open invoices or refund later; refund receipts pay money back straight away.',
  },
  'sales/customers': {
    help: 'Everyone you sell to, with their open receivable balance. Add a customer here or while creating an invoice.',
    guide: 'customers-and-products',
  },
  'sales/products': {
    help: 'Products and services you sell. Choosing one on an invoice or sales receipt fills in its description, rate and income account. Track stock to keep quantity on hand and cost at moving average.',
    guide: 'inventory',
  },
  'sales/finance-charges': {
    help: 'Charge interest on overdue invoices. Pick the assessment date, check or change each customer’s charge, and assess: each becomes an invoice that shows in A/R aging and on statements. Set the rate, minimum, grace days and account in Settings.',
    guide: 'customer-payments',
  },
  'sales/statements': {
    help: 'Statements list each customer’s invoices, payments and credits for a period with a running balance, the amount due and its aging. Select customers and print them, or open one to save it as PDF or email it.',
  },
  'sales/send-forms': {
    guide: 'custom-forms',
  },
};

/**
 * Topics: two sentences each, describing what LocalBooks actually does. `views` lists the
 * screens a topic is about (shown under "On this screen"); `go` opens the place to do it, with an
 * optional `action` (open a form, a Settings tab or section); `guide` is its step-by-step guide.
 */
export const topics = [
  {
    id: 'invoice', area: 'sales', views: ['sales/invoices', 'sales/all', 'sales/customers'],
    title: 'Create an invoice',
    body: 'Fill in the invoice form at the top of Sales › Invoices: type the customer, then add lines with a quantity, rate and income account (decimals such as 1.5 hours are fine). Posting records the amount in Accounts Receivable and income, and the invoice can be printed or saved as a PDF with your form design.',
    keywords: 'bill customer charge billing new print pdf',
    go: { label: 'New invoice', view: 'sales/invoices', action: 'invoice' },
    guide: 'invoices',
  },
  {
    id: 'receive-payment', area: 'sales', views: ['sales/invoices', 'sales/all', 'banking/transfers'],
    title: 'Record a customer payment',
    body: 'Receive Payment applies money to one or several open invoices, in full or in part, and the invoice status updates to Partial or Paid. Deposit it straight to a bank account, or to Undeposited Funds to combine several payments into one bank deposit later.',
    keywords: 'paid payment customer receive partial undeposited',
    go: { label: 'Receive a payment', view: 'sales/invoices', action: 'receive-payment' },
    guide: 'customer-payments',
  },
  {
    id: 'foreign-currency', area: 'sales', views: ['sales/invoices', 'expenses/expenses'],
    title: 'Invoice or pay in another currency',
    body: 'Choose a currency and exchange rate on an invoice, expense or check; your books record the home-currency amount and keep the original alongside. A payment received at a different rate posts the difference to Exchange Gain or Loss automatically.',
    keywords: 'currency fx euro foreign exchange rate gain loss',
    go: { label: 'New invoice', view: 'sales/invoices', action: 'invoice' },
    guide: 'invoices',
  },
  {
    id: 'sales-receipt', area: 'sales', views: ['sales/all'],
    title: 'Record a sale paid on the spot (sales receipt)',
    body: 'Use New sales receipt in All Sales when the customer pays at the time of sale, so no invoice or receivable is needed. Choose whether the money goes to a bank account or to Undeposited Funds.',
    keywords: 'cash sale receipt point of sale paid',
    go: { label: 'New sales receipt', view: 'sales/all', action: 'sales-receipt' },
    guide: 'sales-receipts-and-estimates',
  },
  {
    id: 'estimate', area: 'sales', views: ['sales/estimates'],
    title: 'Quote a customer with an estimate',
    body: 'Create an estimate the same way as an invoice; it does not affect your books. When the customer agrees, Edit as invoice loads its lines into the invoice form to review before posting, and the estimate is marked converted when the invoice posts.',
    keywords: 'quote proposal bid estimate convert accept',
    go: { label: 'New estimate', view: 'sales/estimates', action: 'estimate' },
    guide: 'sales-receipts-and-estimates',
  },
  {
    id: 'customers', area: 'sales', views: ['sales/customers'],
    title: 'Add and review customers',
    body: 'Add a customer with a name and optional email, and see each customer’s open receivable balance. Customers are also available when creating invoices, estimates and billable time.',
    keywords: 'customer client contact receivable balance',
    go: { label: 'Go to Customers', view: 'sales/customers' },
    guide: 'customers-and-products',
  },
  {
    id: 'products', area: 'sales', views: ['sales/products', 'sales/invoices'],
    title: 'Set up products and services',
    body: 'Add each product or service with its rate and the income account it belongs to. Picking it on an invoice or sales receipt then fills in the description, rate and account for you.',
    keywords: 'item service product price rate catalog',
    go: { label: 'Go to Products & Services', view: 'sales/products' },
    guide: 'customers-and-products',
  },

  {
    id: 'finance-charges', area: 'sales', views: ['sales/finance-charges', 'sales/invoices', 'sales/statements', 'settings'],
    title: 'Charge interest on overdue invoices',
    body: 'Set an annual rate, minimum charge, grace days and income account under Settings › Finance charges, then open Sales › Assess Finance Charges. Each ticked customer gets a finance charge invoice for interest on their overdue balance, which appears in A/R aging and on statements.',
    keywords: 'finance charge late fee interest overdue past due assess penalty',
    go: { label: 'Assess finance charges', view: 'sales/finance-charges' },
    guide: 'customer-payments',
  },
  {
    id: 'change-prices', area: 'sales', views: ['sales/products'],
    title: 'Change many prices at once',
    body: 'On Sales › Products & Services, click Change prices… and tick the items. Raise or lower them by a percentage or a fixed amount, based on the current price or the unit cost, and round the result; check the new prices, then apply. Documents already written keep their prices, and each change is recorded in the audit trail.',
    keywords: 'price increase bulk change prices markup percent raise lower round item prices',
    go: { label: 'Go to Products & Services', view: 'sales/products' },
    guide: 'customers-and-products',
  },
  {
    id: 'price-levels', area: 'sales', views: ['sales/products', 'sales/customers', 'sales/invoices', 'sales/all', 'sales/estimates', 'reports'],
    title: 'Give customers their own prices with price levels',
    body: 'On Sales › Products & Services, click Price levels… to make a level: Fixed % raises or lowers every standard price by a percentage; Per item gives chosen items a fixed price or a percentage of their standard price or cost. Both round as you choose. Set a customer’s Price level on their record, and their new invoices, sales receipts and estimates fill in its prices; each line’s Price level can be changed, and a rate you type always wins. Reports › Item Price List shows every item at every level. Changes are recorded in the audit trail.',
    keywords: 'price level pricing wholesale discount customer price list special prices per item markup cost percent item price list',
    go: { label: 'Go to Products & Services', view: 'sales/products' },
    guide: 'customers-and-products',
  },
  {
    id: 'inventory', area: 'sales', views: ['sales/products', 'expenses/bills', 'sales/invoices'],
    title: 'Track inventory and cost of goods sold',
    body: 'Choose Track stock on a product, record opening stock with Adjust stock, and buy more on bills by picking it as the Stock item. Selling it on an invoice or sales receipt records its moving average cost in Cost of Goods Sold; you can sell before the bill arrives, and the cost is trued up when it does.',
    keywords: 'inventory stock quantity on hand cogs cost of goods sold average cost sku count adjust',
    go: { label: 'Go to Products & Services', view: 'sales/products' },
    guide: 'inventory',
  },

  {
    id: 'sales-tax', area: 'sales', views: ['sales/tax', 'sales/invoices', 'sales/all', 'sales/products', 'sales/customers'],
    title: 'Charge and pay sales tax',
    body: 'Turn on Charge sales tax under Sales › Sales Tax and add your rates. Sales forms then show a Tax column (filled from each product’s Taxable setting) and a tax rate (from the customer, or none if they are exempt); the tax goes to Sales Tax Payable. Pay sales tax records what you send the agency, and Reports › Sales Tax Liability shows taxable sales and tax owed.',
    keywords: 'sales tax vat gst tax rate taxable exempt pay sales tax liability agency',
    go: { label: 'Go to Sales Tax', view: 'sales/tax' },
  },
  {
    id: 'credit-memo', area: 'sales', views: ['sales/credits', 'sales/invoices', 'sales/customers'],
    title: 'Give a customer credit or a refund',
    body: 'Sales › Credit Memos & Refunds: a credit memo reduces what the customer owes and can be applied to their open invoices or refunded by check later; a refund receipt pays the money back straight away. Applying a credit marks the invoice paid by that amount.',
    keywords: 'credit memo refund return customer credit refund receipt apply credit',
    go: { label: 'New credit memo', view: 'sales/credits', action: 'credit-memo' },
  },
  {
    id: 'vendor-credit', area: 'expenses', views: ['expenses/credits', 'expenses/bills', 'expenses/vendors'],
    title: 'Record a vendor credit',
    body: 'Expenses › Vendor Credits records credit a vendor gave you for returns or overcharges. Apply it to that vendor’s open bills, or record the refund when they send money back.',
    keywords: 'vendor credit supplier credit return refund debit memo',
    go: { label: 'New vendor credit', view: 'expenses/credits', action: 'vendor-credit' },
  },
  {
    id: 'statements', area: 'sales', views: ['sales/statements', 'sales/customers', 'sales/invoices'],
    title: 'Send customer statements',
    body: 'Sales › Customer Statements lists customers with a balance or activity in the period. Click one to preview their statement (balance forward, each invoice, payment and credit, the amount due and its aging), tick several to print them together, or use Print…, Save as PDF… or Email… above the preview to send one.',
    keywords: 'statement customer statement balance forward account summary aging dunning email',
    go: { label: 'Go to Customer Statements', view: 'sales/statements' },
  },
  {
    id: 'jobs', area: 'sales', views: ['sales/customers', 'sales/invoices'],
    title: 'Track jobs for a customer',
    body: 'A job is a customer under another customer, written Customer:Job (for example Acme:Kitchen remodel). Choose Add job… on a customer, or type the full name on any sales form; statements include a customer’s jobs.',
    keywords: 'job project sub-customer subcustomer customer job',
    go: { label: 'Go to Customers', view: 'sales/customers' },
  },
  {
    id: 'purchase-orders', area: 'expenses', views: ['expenses/purchase-orders', 'expenses/bills'],
    title: 'Order from a vendor with a purchase order',
    body: 'Expenses › Purchase Orders records what you ordered without posting anything. When the bill arrives, choose Create bill… to post it with the same lines; close an order you no longer expect.',
    keywords: 'purchase order po order vendor supplier non-posting receive items',
    go: { label: 'New purchase order', view: 'expenses/purchase-orders', action: 'purchase-order' },
  },
  {
    id: 'recurring', area: 'accounting', views: ['accounting/recurring', 'expenses/expenses', 'expenses/bills', 'sales/invoices', 'accounting/journal'],
    title: 'Memorize a recurring transaction',
    body: 'Fill in an expense, check, bill, invoice or journal entry and choose Memorize… to save it with a schedule. Accounting › Recurring Transactions enters everything that is due in one step; a reminder appears at the top of the window when something is due.',
    keywords: 'recurring memorize memorized repeat schedule subscription rent monthly automatic',
    go: { label: 'Go to Recurring Transactions', view: 'accounting/recurring' },
  },
  {
    id: 'budgets', area: 'accounting', views: ['accounting/budgets', 'reports'],
    title: 'Set a budget and compare it with actuals',
    body: 'Accounting › Budgets holds expected income and expenses per account for each month of a fiscal year, and optionally the month-end balance of balance sheet accounts; Copy across repeats the first month. Reports › Profit and Loss Budget vs. Actual and Balance Sheet Budget vs. Actual show actual, budget, the amount over budget and the percent of budget.',
    keywords: 'budget forecast plan budget vs actual variance balance sheet budget',
    go: { label: 'Go to Budgets', view: 'accounting/budgets' },
    guide: 'reports',
  },
  {
    id: 'edit-transaction', area: 'accounting', views: ['sales/invoices', 'expenses/bills', 'expenses/expenses', 'accounting/journal'],
    title: 'Edit a posted transaction',
    body: 'Choose Edit… on an invoice, bill, expense, check or manual journal entry: the form opens with its details and Save changes replaces it. The original is voided and the edited version posted in one step, so both stay in the audit trail; remove payments or credits first.',
    keywords: 'edit change correct fix modify posted transaction',
    go: { label: 'Go to Journal Entries', view: 'accounting/journal' },
  },
  {
    id: 'attachments', area: 'accounting', views: ['accounting/journal', 'accounting/register'],
    title: 'Attach receipts and documents',
    body: 'Open a transaction’s lines in the journal or an account register and choose Attach file… to keep a copy of a receipt or contract in the company file (up to 25 MB each). Click a file name to save a copy.',
    keywords: 'attachment attach receipt document file scan photo pdf',
    go: { label: 'Go to Journal Entries', view: 'accounting/journal' },
  },
  {
    id: '1099', area: 'expenses', views: ['expenses/vendors', 'reports'],
    title: 'Prepare 1099s',
    body: 'Mark contractors Track payments for a 1099 and add their tax ID on the vendor (Expenses › Vendors › Edit…). Reports › 1099 Summary totals what you paid each of them in a calendar year by check, cash or bank transfer and flags those over $600.',
    keywords: '1099 1099-nec contractor tax id ein ssn year end vendor',
    go: { label: 'Go to Vendors', view: 'expenses/vendors' },
  },
  {
    id: 'search', area: 'accounting', views: ['accounting/journal', 'accounting/find'],
    title: 'Find a transaction',
    body: 'Type in the search box at the top of the window (⌘K or Ctrl+K) to find transactions by number, name, memo, reference or exact amount, and customers, vendors, products and accounts by name. Choose a result to open it. For more, choose Advanced search… at the bottom of the results, or Accounting › Find Transactions: narrow by dates, transaction type, customer or vendor, account, amount (equal to, at least, at most or between) and number, with a count and total of what matches.',
    keywords: 'search find lookup locate amount check number invoice number advanced filter',
    go: { label: 'Go to Find Transactions', view: 'accounting/find' },
    guide: 'journal-entries-and-voids',
  },
];
