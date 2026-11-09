// Help, part 4: topics on Banking (the number in the file name sets the order) and the help for its screens.
// Each topic’s own `area` decides its heading under All topics.

/** What each screen is for (`help`) and its main guide (`guide`, docs/guide/<id>.md), shown at the top of Help. */
export const screens = {
  'banking/feeds': {
    help: 'Import your bank’s CSV, OFX or QFX statement, or connect a bank through SimpleFIN to sync new transactions, then approve each transaction into a category. LocalBooks skips anything already imported and learns from what you approve.',
    guide: 'bank-feeds',
  },
  'banking/transfers': {
    help: 'Bank deposits (including customer payments held in Undeposited Funds) and transfers between your own accounts.',
    guide: 'deposits-and-transfers',
  },
  'banking/bounced-checks': {
    help: 'Record a customer’s check that the bank returned: the invoice it paid opens again, the amount and any bank fee come out of the bank, and a returned-check fee can be charged to the customer on a new invoice.',
    guide: 'customer-payments',
  },
  'banking/reconcile': {
    help: 'Match your books to a bank or card statement and lock the matched transactions so the reconciled balance can never change.',
    guide: 'reconciling',
  },
};

/**
 * Topics: two sentences each, describing what LocalBooks actually does. `views` lists the
 * screens a topic is about (shown under "On this screen"); `go` opens the place to do it, with an
 * optional `action` (open a form, a Settings tab or section); `guide` is its step-by-step guide.
 */
export const topics = [
  {
    id: 'bank-feed', area: 'banking', views: ['banking/feeds'],
    title: 'Import and categorize bank transactions',
    body: 'Import a CSV, OFX or QFX statement from your bank, or click Connect a bank… to fetch transactions through SimpleFIN (overlapping imports and syncs are fine: anything already imported is skipped), then approve each transaction into a category or exclude it. Approved lines become real expenses or deposits, and LocalBooks learns from your choices to suggest a category next time. A downloaded line that is a check, deposit or payment you already entered is matched to it instead (on import when only one agrees, or with Match / Find a match…), and Show matched lists those, with Unmatch. Tick several lines to add them at once.',
    keywords: 'bank feed csv ofx qfx qbo import statement categorize approve exclude rule download duplicate columns mapping simplefin connect sync automatic setup token match unmatch matched already entered add several batch',
    go: { label: 'Go to Bank Feeds', view: 'banking/feeds' },
    guide: 'bank-feeds',
  },
  {
    id: 'deposit', area: 'banking', views: ['banking/transfers', 'sales/invoices'],
    title: 'Make a bank deposit',
    body: 'Choose the customer payments waiting in Undeposited Funds that went into the bank together, and add any other money in the same deposit, with the customer or vendor it was received from. Cash back takes part of it out as cash to an account such as Petty Cash. The deposit total then matches the single line on your bank statement.',
    keywords: 'deposit undeposited funds bank payments combine cash back petty cash received from payee',
    go: { label: 'New bank deposit', view: 'banking/transfers', action: 'bank-deposit' },
    guide: 'deposits-and-transfers',
  },
  {
    id: 'transfer', area: 'banking', views: ['banking/transfers'],
    title: 'Move money between your accounts',
    body: 'A transfer moves money between two of your own accounts, such as checking to savings or paying down a credit card. It is not income or an expense, so it does not appear on Profit and Loss.',
    keywords: 'transfer move savings checking credit card',
    go: { label: 'New transfer', view: 'banking/transfers', action: 'transfer' },
    guide: 'deposits-and-transfers',
  },
  {
    id: 'pay-down-card', area: 'banking', views: ['banking/transfers', 'expenses/expenses'],
    title: 'Pay down a credit card',
    body: 'Banking › Pay Down Credit Card… pays a card from a bank account: choose the card, and the amount starts at what the card owes. It posts as a transfer (the card goes down, the bank goes down), so it shows in both registers and reconciles against both statements.',
    keywords: 'pay credit card balance payment paydown pay off card bill',
    go: { label: 'Pay down a card', view: 'banking/transfers', action: 'pay-card' },
    guide: 'deposits-and-transfers',
  },
  {
    id: 'bounced-check', area: 'banking', views: ['banking/bounced-checks', 'sales/invoices', 'banking/transfers'],
    title: 'Record a bounced check',
    body: 'When the bank returns a customer’s check, choose it in Banking › Record Bounced Check… and enter the bank’s fee and any fee you charge the customer. The invoice it paid opens again, the bank goes down by the check and the fee, and your fee goes on a new invoice; voiding the bounced check puts it all back.',
    keywords: 'bounced check nsf returned insufficient funds rubber check fee',
    go: { label: 'Record a bounced check', view: 'banking/bounced-checks' },
    guide: 'customer-payments',
  },
  {
    id: 'reconcile', area: 'banking', views: ['banking/reconcile', 'banking/feeds'],
    title: 'Reconcile a bank or card account',
    body: 'Enter the statement end date and ending balance, then tick the transactions that cleared until the difference is $0.00. Finishing locks those lines, so the reconciled balance can never drift.',
    keywords: 'reconcile statement match cleared balance',
    go: { label: 'Go to Reconcile', view: 'banking/reconcile' },
    guide: 'reconciling',
  },
];
