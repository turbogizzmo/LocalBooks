// Help, part 2: topics on Expenses & vendors (the number in the file name sets the order) and the help for its screens.
// Each topic’s own `area` decides its heading under All topics.

/** What each screen is for (`help`) and its main guide (`guide`, docs/guide/<id>.md), shown at the top of Help. */
export const screens = {
  'expenses/credits': {
    help: 'Credit a vendor gave you, for returns or overcharges: apply it to their open bills, or record the refund they send.',
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
  'expenses/print-checks': {
    help: 'Checks marked Print later, waiting to print. Choose the bank account, enter the number on the first check in the printer, and print: the checks take numbers in order, and only the ones you confirm printed keep them.',
    guide: 'expenses-and-checks',
  },
  'expenses/purchase-orders': {
    help: 'Orders you send vendors. A purchase order does not touch your books; when the goods or bill arrive, turn it into a bill with the same lines in one step.',
  },
};

/**
 * Topics: two sentences each, describing what LocalBooks actually does. `views` lists the
 * screens a topic is about (shown under "On this screen"); `go` opens the place to do it, with an
 * optional `action` (open a form, a Settings tab or section); `guide` is its step-by-step guide.
 */
export const topics = [
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
    id: 'pay-bills', area: 'expenses', views: ['expenses/bills', 'expenses/vendors'],
    title: 'Pay bills',
    body: 'Pay bills lists every open bill so you can pay several at once, fully or partly, from a bank or credit card account. Each payment reduces Accounts Payable and the account it was paid from. Paying from a bank account writes one check per vendor, numbered from the account\'s next check number, and Print checks prints them.',
    keywords: 'pay vendor payable ap batch partial',
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
];
