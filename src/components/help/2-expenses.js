// Help, part 2: topics on Expenses & vendors (the number in the file name sets the order) and the help for its screens.
// Each topic’s own `area` decides its heading under All topics.

/** What each screen is for (`help`) and its main guide (`guide`, docs/guide/<id>.md), shown at the top of Help. */
export const screens = {
  'expenses/credits': {
    help: 'Credit a vendor gave you, for returns or overcharges: apply it to their open bills, or record the refund they send.',
    guide: 'bills',
  },
  'expenses/expenses': {
    help: 'Money paid straight away: expenses (card, cash or bank) and checks, each split across one or more expense accounts.',
    guide: 'expenses-and-checks',
  },
  'expenses/bills': {
    help: 'Bills you owe vendors, tracked in Accounts Payable until paid. Pay several at once with Pay bills.',
    guide: 'bills',
  },
  'expenses/vendors': {
    help: 'Everyone you buy from, with what you still owe each of them.',
    guide: 'customers-and-products',
  },
  'expenses/other-names': {
    help: 'People and businesses who are neither customers nor vendors, such as an owner taking a draw. Checks and expenses can be paid to them.',
    guide: 'customers-and-products',
  },
  'expenses/inbox': {
    help: 'Receipts and bills waiting to be entered. Drop files on the window or click Add files…; each shows a preview, and Enter as bill, Expense or Check opens that form filled in from what could be read from the file, with the file attached when it posts.',
    guide: 'attachments',
  },
  'expenses/print-checks': {
    help: 'Checks marked Print later, waiting to print. Choose the bank account, enter the number on the first check in the printer, and print: the checks take numbers in order, and only the ones you confirm printed keep them.',
    guide: 'expenses-and-checks',
  },
  'expenses/purchase-orders': {
    help: 'Orders you send vendors. A purchase order does not touch your books; when the goods or bill arrive, turn it into a bill with the same lines (and each line’s Customer:Job) in one step, or receive the goods first with Receive Items.',
    guide: 'purchase-orders',
  },
  'expenses/item-receipts': {
    help: 'Goods received from a purchase order before the vendor’s bill: the stock is on hand, and its cost waits in Items Received Not Billed until you enter the bill here.',
    guide: 'bills',
  },
  'expenses/approvals': {
    help: 'Rules that hold bills and purchase orders over an amount, or from vendors you choose, until an Admin approves them, and the list of what is waiting. A waiting bill isn’t posted or in Pay Bills; approving records it as entered, and rejecting keeps it with a note.',
    guide: 'bills',
  },
};

/**
 * Topics: two sentences each, describing what LocalBooks actually does. `views` lists the
 * screens a topic is about (shown under "On this screen"); `go` opens the place to do it, with an
 * optional `action` (open a form, a Settings tab or section); `guide` is its step-by-step guide.
 */
export const topics = [
  {
    id: 'other-names', area: 'expenses', views: ['expenses/other-names', 'expenses/expenses'],
    title: 'Other names: payees who aren’t vendors',
    body: 'Expenses › Other Names keeps people and businesses who are neither customers nor vendors, such as an owner taking a draw. Add one with New other name; then type or pick the name as the payee on a check or expense and it is paid to them, with no vendor added. Click a name for its panel: what has been paid to them, contact details, notes with Date Stamp and recent transactions.',
    keywords: 'other names other name list owner draw partner payee not a vendor one-off payee information panel notes',
    go: { label: 'Go to Other Names', view: 'expenses/other-names' },
    guide: 'customers-and-products',
  },
  {
    id: 'receive-items', area: 'expenses', views: ['expenses/item-receipts', 'expenses/purchase-orders'],
    title: 'Receive items before the bill',
    body: 'Vendors › Receive Items brings goods from an open purchase order into stock before the bill comes; their cost waits in Items Received Not Billed, not Accounts Payable. When the bill arrives, Enter Bill for Received Items posts it at the vendor’s prices, and any price difference goes to the stock, Cost of Goods Sold or the line’s expense account. Each line’s Customer:Job from the purchase order carries through to the bill.',
    keywords: 'item receipt receive items received not billed packing slip partial shipment enter bill for received items purchase order',
    go: { label: 'Go to Receive Items', view: 'expenses/item-receipts' },
    guide: 'bills',
  },
  {
    id: 'document-inbox', area: 'expenses', views: ['expenses/inbox', 'expenses/bills', 'expenses/expenses'],
    title: 'Enter receipts and bills from the Document Inbox',
    body: 'Expenses › Document Inbox keeps receipts and vendor bills until you enter them: drop files on the window or click Add files…. Each shows a preview, and for a PDF with text LocalBooks reads the total, the date and the name at the top. Enter as bill, Expense or Check opens that form filled in, and the file is attached to the transaction when it posts. Files stay in the company file; nothing is sent anywhere, and scans are not read (no OCR).',
    keywords: 'document inbox receipt bill upload review scan pdf photo drop attach enter receipts receipt management',
    go: { label: 'Go to Document Inbox', view: 'expenses/inbox' },
    guide: 'attachments',
  },
  {
    id: 'print-checks-later', area: 'expenses', views: ['expenses/expenses', 'expenses/print-checks'],
    title: 'Print checks later, in a batch',
    body: 'Tick Print later on Write Checks and the check is saved without a number, showing To print in the list. File › Print Forms › Checks… prints the waiting checks in order from the first number on your check stock, then asks whether they all printed: the ones that did keep their numbers, the others stay queued. Pay Bills has Print later too: its checks, one per vendor, wait in the same queue.',
    keywords: 'print later to be printed queue batch check numbers print forms pay bills',
    go: { label: 'Go to Print Checks', view: 'expenses/print-checks' },
    guide: 'expenses-and-checks',
  },
  {
    id: 'expense-check', area: 'expenses', views: ['expenses/expenses'],
    title: 'Record an expense or write a check',
    body: 'Record money already spent as an expense (from a bank or credit card) or a check (numbered automatically), split across as many expense accounts as needed. It posts immediately and reduces the account it was paid from. Ending balance beside the account box shows what it holds (or a card owes) now.',
    keywords: 'spend purchase receipt check cheque card payment split',
    go: { label: 'New expense', view: 'expenses/expenses', action: 'expense' },
    guide: 'expenses-and-checks',
  },
  {
    id: 'card-credit', area: 'expenses', views: ['expenses/expenses'],
    title: 'Record a credit card credit',
    body: 'Record a refund or credit from a vendor onto a credit card as a card credit: the reverse of a card charge. It lowers the card balance and takes the amount back out of each expense category on its lines.',
    keywords: 'refund return credit card credit ccard refund chargeback reversal vendor refund',
    go: { label: 'New card credit', view: 'expenses/expenses', action: 'card-credit' },
    guide: 'expenses-and-checks',
  },
  {
    id: 'bill', area: 'expenses', views: ['expenses/bills', 'expenses/vendors'],
    title: 'Enter a vendor bill',
    body: 'Choose New bill, pick the vendor and split the total across expense accounts, by amount or by quantity × rate; it must add up to the bill total. The bill sits in Accounts Payable until you pay it. With a discount term (such as 2% 10 Net 30), Discount date shows the last day to pay and take the discount.',
    keywords: 'vendor payable ap supplier invoice split owe',
    go: { label: 'New bill', view: 'expenses/bills', action: 'bill' },
    guide: 'bills',
  },
  {
    id: 'purchase-discounts', area: 'expenses', views: ['expenses/bills', 'expenses/expenses', 'expenses/credits', 'expenses/purchase-orders', 'sales/products'],
    title: 'Take a discount or add a charge on a bill, check or purchase order',
    body: 'Under the split lines, click Add a discount or charge, choose the split line it follows, and enter an amount or a percent of that line such as 2%. Picking a Discount or Other Charge item fills in its account and amount. A discount comes off the total and a charge is added. A purchase order\'s discounts and charges carry over to its bill. An Other Charge or Discount item can have its own expense account for bills and checks (Edit the item).',
    keywords: 'discount charge freight shipping other charge item purchase bill check expense vendor credit purchase order po percent expense account',
    go: { label: 'Go to Bills', view: 'expenses/bills' },
    guide: 'bills',
  },
  {
    id: 'approvals', area: 'expenses', views: ['expenses/approvals', 'expenses/bills', 'expenses/purchase-orders'],
    title: 'Approve bills and purchase orders',
    body: 'Under Expenses › Approvals an Admin adds rules, such as bills of $1,000 or more, or every bill from one vendor. A bill or purchase order that matches is sent for approval instead of being recorded: a waiting bill isn’t posted and isn’t in Pay Bills, and a waiting order isn’t an open order. An Admin approves it, which records it as entered, or rejects it with a note, which keeps the request and records nothing.',
    keywords: 'approval approve reject rule authorize authorise sign off manager bill purchase order po pending waiting threshold',
    go: { label: 'Go to Approvals', view: 'expenses/approvals' },
    guide: 'bills',
  },
  {
    id: 'pay-bills', area: 'expenses', views: ['expenses/bills', 'expenses/vendors'],
    title: 'Pay bills',
    body: 'Pay bills lists every open bill (Filter by a vendor, Sort by due date, discount date, vendor or amount; click one for its details and Go to Bill) so you can pay several at once, fully or partly, from a bank or credit card account. Each payment reduces Accounts Payable and the account it was paid from. Paying from a bank account writes one check per vendor, numbered from the account\'s next check number, and Print checks prints them. Afterwards, Payment stubs lists each vendor paid: print, save or email them a stub of the bills the payment covers. A paid bill\'s Actions menu has the same stub actions later.',
    keywords: 'pay vendor payable ap batch partial stub remittance',
    go: { label: 'Pay bills', view: 'expenses/bills', action: 'pay-bills' },
    guide: 'bills',
  },
  {
    id: 'vendors', area: 'expenses', views: ['expenses/vendors'],
    title: 'Add and review vendors',
    body: 'Add a vendor with a name and optional email, and see what you still owe each one. Click a vendor to see all of its transactions under the list, and double-click one to open it. On a bill, check or expense, type a name that isn’t on the list and choose Add as a new vendor.',
    keywords: 'vendor supplier payee contact payable balance transactions history add new vendor quick add center',
    go: { label: 'Go to Vendors', view: 'expenses/vendors' },
    guide: 'customers-and-products',
  },
  {
    id: 'pay-bills-set', area: 'expenses', views: ['expenses/bills'],
    title: 'Set the discount or credits on one bill in Pay Bills',
    body: 'Click a bill in Pay Bills to see its terms, discount and credits under the list. Set Discount chooses the early-payment discount to take on it (up to what its terms offer; it needs a discount account in Preferences › Bills), and Set Credits how much of each open vendor credit to use. What you set replaces the automatic preferences for that bill.',
    keywords: 'pay bills set discount set credits early payment discount vendor credit apply one bill go to bill',
    guide: 'bills',
  },
];
