// Help, part 7: topics on Company & data (the number in the file name sets the order) and the help for its screens.
// Each topic’s own `area` decides its heading under All topics.

/** What each screen is for (`help`) and its main guide (`guide`, docs/guide/<id>.md), shown at the top of Help. */
export const screens = {
  dashboard: {
    help: 'Your books at a glance: assets and liabilities over the last 12 months, open receivables by age, a live trial balance that always shows whether debits equal credits, and Reminders of what needs attention today.',
    guide: 'getting-started',
  },
  settings: {
    help: 'Company details and preferences, the closing date, backups and company files, users and passwords, and local AI access.',
    guide: 'getting-started',
  },
  'settings/import': {
    help: 'The Desktop Migration Wizard: bring your books in from QuickBooks Desktop, QuickBooks for Mac, Quicken or GnuCash through a staging sandbox, check them against the original, then post them.',
    guide: 'desktop-migration',
  },
};

/**
 * Topics: two sentences each, describing what LocalBooks actually does. `views` lists the
 * screens a topic is about (shown under "On this screen"); `go` opens the place to do it, with an
 * optional `action` (open a form, a Settings tab or section); `guide` is its step-by-step guide.
 */
export const topics = [
  {
    id: 'new-company', area: 'company', views: ['settings/data'],
    title: 'Create a new company with a suggested chart of accounts',
    body: 'File › New Company… asks for your company details and tax form, then suggests a chart of accounts for your industry from the same 33 industries QuickBooks offers, numbered QuickBooks-style; check or uncheck accounts before saving. Skip uses LocalBooks’ standard chart instead, and either way the accounts can be changed later in the Chart of Accounts.',
    keywords: 'new company setup interview easystep industry chart of accounts starter accounts tax form entity sole proprietor llc partnership corporation s corp nonprofit 990',
    go: { label: 'Go to File & Data Controls', view: 'settings/data' },
    guide: 'getting-started',
  },
  {
    id: 'preferences', area: 'company', views: ['settings/preferences'],
    title: 'Set company details and preferences',
    body: 'The Preferences Console holds your company name, address, logo, fiscal year start, currency symbol, home currency and tax form; each saves as soon as you leave the field. The logo and details appear on printed invoices, estimates and reports; Settings › Custom Forms sets how invoices and estimates look.',
    keywords: 'company name address logo fiscal year currency settings preferences tax form',
    go: { label: 'Go to Preferences', view: 'settings/preferences' },
    guide: 'getting-started',
  },
  {
    id: 'custom-forms', area: 'company', views: ['settings/forms', 'sales/invoices', 'sales/estimates', 'sales/all', 'sales/statements', 'sales/customers'],
    title: 'Design your forms: invoices, estimates, sales receipts and statements',
    body: 'Settings › Custom Forms sets the layout (Classic, Modern or Compact), title, paper size, font, accent color, logo placement, columns, message and terms for printed invoices, estimates, sales receipts and customer statements, with a live preview. Save several styles and pick one per customer in the customer’s record; everyone else gets the default style. Print… and Save as PDF… use the customer’s style.',
    keywords: 'form designer template style layout classic modern compact customize logo color font pdf print invoice estimate sales receipt statement',
    go: { label: 'Go to Custom Forms', view: 'settings/forms' },
    guide: 'custom-forms',
  },
  {
    id: 'send-forms', area: 'sales', views: ['sales/send-forms', 'sales/invoices', 'sales/estimates'],
    title: 'Email forms in a batch (Email later and Send Forms)',
    body: 'Tick Email later when you save an invoice or estimate, or choose Email later in a row’s menu; Unsent only shows them. Sales › Send Forms sends the ones you select: all at once over SMTP, or one message per form in your mail app. Each send is added to the form’s sent history (✉ in the list).',
    keywords: 'email later batch send forms queue unsent history sent emailed',
    go: { label: 'Go to Send Forms', view: 'sales/send-forms' },
    guide: 'custom-forms',
  },
  {
    id: 'smtp', area: 'company', views: ['settings/email'],
    title: 'Send emailed forms from LocalBooks (SMTP)',
    body: 'Settings › Email can send invoices and estimates through your email provider’s SMTP server instead of opening your mail app. Enter the server, security, port, username and password (often an app password), click Test connection (or Send test email), then Save. The password is kept in this computer’s Keychain or Credential Manager.',
    keywords: 'smtp email server gmail outlook app password send mail settings',
    go: { label: 'Email settings', view: 'settings/email' },
    guide: 'custom-forms',
  },
  {
    id: 'email-form', area: 'sales', views: ['sales/invoices', 'sales/estimates', 'sales/all'],
    title: 'Email an invoice or estimate',
    body: 'Choose Email… in a row’s Actions menu, check the address, subject and message, and click Open in mail app: a new message opens in your mail app with the PDF attached for you to send. Set the default subject and message in Settings › Custom Forms; to send directly from LocalBooks instead, set up your email server in Settings › Email.',
    keywords: 'email send mail outlook smtp server attach pdf customer invoice estimate',
    go: { label: 'Email settings', view: 'settings/email' },
    guide: 'custom-forms',
  },
  {
    id: 'closing-date', area: 'company', views: ['settings/preferences', 'accounting/journal'],
    title: 'Close a fiscal or tax period',
    body: 'Set a closing date to lock the books through that date, so nothing dated on or before it can be posted or voided from any screen. Add a passcode to require it for moving or removing the date; every change is kept in the closing history.',
    keywords: 'lock close year end period tax password passcode books',
    go: { label: 'Go to the closing date', view: 'settings/preferences', action: 'closing' },
    guide: 'closing-the-books',
  },
  {
    id: 'users', area: 'company', views: ['settings/users', 'expenses/vendors'],
    title: 'Users, passwords and sensitive data',
    body: 'Turn on users so everyone signs in with their own password and a role (Admin, Standard or View only). Vendor tax IDs are then encrypted and shown in full only to users allowed to see sensitive data. Save the recovery key you are given: it is the only way back in if every Admin forgets their password.',
    keywords: 'user users password sign in login log in role admin permission security encrypt encryption ssn ein tax id sensitive recovery key forgot lock idle remember',
    go: { label: 'Go to Users & Passwords', view: 'settings/users' },
    guide: 'users-and-passwords',
  },
  {
    id: 'backup', area: 'company', views: ['settings/data'],
    title: 'Back up, restore or open a company file',
    body: 'File & Data Controls backs up your books to a .localbooks file, restores a backup (keeping a safety copy of the current books) or opens another company file; a daily backup is also taken automatically. Restoring replaces the whole company file, so anything entered after the backup was taken is not kept. Choosing a QuickBooks .qbw, .qbb or .qbm file instead starts a new company and opens the Desktop Migration Wizard on it. Hold Option or Alt while clicking Open (or choose File › Open Company Without Restoring Windows…) to open on a plain Home page with other windows closed.',
    keywords: 'backup restore export file save localbooks open copy qbw qbb qbm quickbooks without restoring windows alt option',
    go: { label: 'Go to File & Data Controls', view: 'settings/data' },
    guide: 'backups-and-company-files',
  },
  {
    id: 'condense', area: 'company', views: ['settings/data'],
    title: 'Condense data: archive closed years',
    body: 'Condense data in File & Data Controls replaces closed transactions up to a cut-off with one summary entry per month, keeping anything still open (unpaid invoices and bills, unused credits, undeposited payments, uncleared bank lines). It first saves a verified archive of the full history, which opens read-only for reports, and a Before condense backup to undo with. The condensed file is checked against the archive before LocalBooks switches to it. Set a closing date and reconcile through the cut-off first.',
    keywords: 'condense archive shrink smaller file old years history summary summarize clean up purge size',
    go: { label: 'Go to File & Data Controls', view: 'settings/data' },
    guide: 'backups-and-company-files',
  },
  {
    id: 'file-location', area: 'company', views: ['settings/data'],
    title: 'Keep the company file on a network or synced drive',
    body: 'Change Company File Location moves your books to another folder, including a network drive or a synced folder such as Dropbox. Only one person can have the books open at a time, and LocalBooks tells a second computer they are in use.',
    keywords: 'network drive share dropbox icloud onedrive location move folder',
    go: { label: 'Go to File & Data Controls', view: 'settings/data' },
    guide: 'backups-and-company-files',
  },
  {
    id: 'mcp', area: 'company', views: ['settings/ai', 'reports', 'dashboard'],
    title: 'Connect Claude, Cursor or another AI assistant (local MCP)',
    body: 'Turn on AI Access (MCP) to let an assistant on this computer read your trial balance, profit and loss summary and overdue receivables; it cannot change anything, and it answers only on this computer. Copy the ready-made Claude Code command, or the JSON configuration for clients such as Claude Desktop or Cursor.',
    keywords: 'ai claude cursor mcp model context protocol assistant chatgpt token llm',
    go: { label: 'Go to AI Access', view: 'settings/ai' },
    guide: 'ai-assistant',
  },
  {
    id: 'qbw-import', area: 'company', views: ['settings/import'],
    title: 'Import a desktop company file (.qbw)',
    body: 'Choose a .qbw file: LocalBooks reads it without changing it and brings in invoice line amounts only, because customers, payments and account names cannot be read from .qbw files yet. Map each line type to an income account, compare the total with the original program, then post.',
    keywords: 'qbw qbb qbm migrate migration wizard desktop company file convert move import extractor',
    go: { label: 'Go to the Desktop Migration Wizard', view: 'settings/import' },
    guide: 'desktop-migration',
  },
  {
    id: 'qbmac-import', area: 'company', views: ['settings/import'],
    title: 'Import a QuickBooks for Mac 2012 (or earlier) company file',
    body: 'Choose the .qb2012, .qb2011 or .qb2010 company file in the Desktop Migration Wizard: LocalBooks reads accounts, customers and jobs, vendors, items and every transaction, with the invoices and bills each payment paid. Files from QuickBooks for Mac 2016 and later are encrypted, so save those with File › Export › To QuickBooks for Windows and choose the .qbb file instead.',
    keywords: 'quickbooks mac qb2012 qb2011 qb2010 macintosh company file import migrate encrypted qbmac',
    go: { label: 'Go to the Desktop Migration Wizard', view: 'settings/import' },
    guide: 'desktop-migration',
  },
  {
    id: 'qb-installed-export', area: 'company', views: ['settings/import'],
    title: 'Export from my installed QuickBooks (Preview)',
    body: 'If QuickBooks Desktop still runs on this computer, open your company file in it, then open "QuickBooks still installed? Export from my installed QuickBooks" under step 1 and click Export from QuickBooks. LocalBooks asks QuickBooks for the chart of accounts, customers, vendors, items and Journal through Intuit\'s own interfaces, sending read-only requests only, and stages them like report exports. On a Mac, first open Reports › Accountant & Taxes › Journal with the dates set to All and leave it in front; the first time, macOS asks whether LocalBooks may control QuickBooks: click OK (or turn it on later in System Settings › Privacy & Security › Automation). On Windows, install Intuit\'s free QuickBooks Desktop SDK, open the file as Admin, and when QuickBooks asks about LocalBooks choose "Yes, whenever this QuickBooks company file is open". Preview: not yet tried against a running QuickBooks; if it fails, export the reports yourself.',
    keywords: 'quickbooks installed running export qbxml sdk applescript automation permission certificate mac windows migrate preview',
    go: { label: 'Go to the Desktop Migration Wizard', view: 'settings/import' },
    guide: 'desktop-migration',
  },
  {
    id: 'qbw-undo', area: 'company', views: ['settings/import'],
    title: 'Undo or delete a desktop migration import',
    body: 'Reverse posted entries voids everything an import posted and deactivates the accounts it created, returning your balances to where they were. Delete import removes only the staged lines and never touches your books.',
    keywords: 'undo reverse delete import qbw remove staged',
    go: { label: 'Go to the Desktop Migration Wizard', view: 'settings/import' },
    guide: 'desktop-migration',
  },
  {
    id: 'shortcuts', area: 'company', views: [],
    title: 'Keyboard shortcuts',
    body: 'Press Ctrl+Alt+N (⌘⌥N on a Mac) to open + New from anywhere, Ctrl+Shift+N (⇧⌘N) to open a new window, and F1 to open or close Help. Esc closes Help and other panels.',
    keywords: 'keyboard shortcut hotkey keys f1 new window',
    go: null,
    guide: 'troubleshooting',
  },
  {
    id: 'sort-lists', area: 'company', views: [],
    title: 'Sort a list by any column',
    body: 'Click a column heading in a list such as invoices, bills, customers or the journal to sort by it, and click it again to reverse the order. An arrow marks the sorted column.',
    keywords: 'sort order column heading header ascending descending arrange',
    go: null,
    guide: 'getting-started',
  },
  {
    id: 'menu-bar', area: 'company', views: [],
    title: 'Use the QuickBooks-style menu bar',
    body: 'The menu bar has the classic Lists, Company, Customers, Vendors, Employees, Banking and Reports menus; each item opens its screen or form, and Reports lists every report by group. Shortcuts: Ctrl+I invoice, Ctrl+W write checks (not on a Mac), Ctrl+R register, Ctrl+J Customer Center, Ctrl+T memorized transactions, Ctrl+F find (⌘ on a Mac).',
    keywords: 'menu menus menu bar lists company customers vendors employees banking reports shortcut ctrl+i ctrl+w ctrl+r ctrl+j ctrl+t ctrl+f quickbooks desktop',
    go: null,
    guide: 'getting-started',
  },
  {
    id: 'windows', area: 'company', views: [],
    title: 'Work in more than one window',
    body: 'Choose New window at the top of any screen (or Window › New Window, Ctrl+Shift+N / ⇧⌘N) to open that screen in another window on the same company; registers and reports also have Open in new window. Every window shows the same books, and a change saved in one appears in the others straight away.',
    keywords: 'window windows multiple new window side by side second monitor register report open',
    go: null,
    guide: 'getting-started',
  },
];
