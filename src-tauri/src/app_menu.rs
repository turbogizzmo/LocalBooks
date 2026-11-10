//! The rest of the menu bar, laid out like classic desktop accounting software: View, Lists,
//! Company, Customers, Vendors, Employees, Banking and Reports, plus the extra File, Edit and Help
//! items. (The company items at the top of File are `file_menu`'s, Window › New Window `windows`'.)
//!
//! Every item opens a page of the app, often with an entry form or report: the focused window is sent
//! `menu-navigate` with `{ view, action, data }`, as the app's own navigation passes them. A few items
//! act on the page itself (show Help, hide the side bar) and are sent as
//! `menu-command`. Shortcuts follow the classic ones where they don't clash with text editing or the
//! platform (Ctrl+A stays Select All; Cmd+W stays Close Window on macOS).

use serde_json::json;
use tauri::menu::{IsMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID};
use tauri::{AppHandle, Emitter, Wry};

/// Sent to the focused window with the page to open.
const MENU_NAVIGATE: &str = "menu-navigate";
/// Sent to the focused window with a command for the page ("help", "sidebar").
const MENU_COMMAND: &str = "menu-command";
/// A navigation item's id: this prefix, then `view|action|report|n` (n keeps repeated items unique).
const GO_PREFIX: &str = "go:";
/// A page command's id: this prefix, then the command.
const CMD_PREFIX: &str = "cmd:";

enum Entry {
    /// Opens `view`, optionally with an entry form (`action`) or a report.
    Go { label: &'static str, view: &'static str, action: Option<&'static str>, report: Option<&'static str>, accel: Option<&'static str> },
    Cmd { label: &'static str, command: &'static str, accel: Option<&'static str> },
    Sub(&'static str, &'static [Entry]),
    Sep,
}

const fn page(label: &'static str, view: &'static str) -> Entry {
    Entry::Go { label, view, action: None, report: None, accel: None }
}

const fn form(label: &'static str, view: &'static str, action: &'static str) -> Entry {
    Entry::Go { label, view, action: Some(action), report: None, accel: None }
}

const fn keyed(label: &'static str, view: &'static str, action: Option<&'static str>, accel: &'static str) -> Entry {
    Entry::Go { label, view, action, report: None, accel: Some(accel) }
}

const fn report(label: &'static str, id: &'static str) -> Entry {
    Entry::Go { label, view: "reports", action: Some("open"), report: Some(id), accel: None }
}

const ITEM_LIST: Entry = page("Item List", "sales/products");
const CHART_OF_ACCOUNTS: Entry = page("Chart of Accounts", "accounting/chart-of-accounts");

/// Added to File below the company items.
const FILE: &[Entry] = &[
    Entry::Sub("Back Up Company", &[form("Create Local Backup…", "settings", "data")]),
    // Import / Convert… opens the Desktop Migration Wizard (Settings › Import & Migration).
    Entry::Sub(
        "Utilities",
        &[
            page("Import / Convert…", "settings/import"),
            Entry::Sub("Export", &[form_cmd("Lists to IIF Files…", "export-lists-iif"), form_cmd("Addresses to Text File…", "export-addresses")]),
            form("Condense Data…", "settings", "data"),
        ],
    ),
    Entry::Sep,
    Entry::Sub(
        "Print Forms",
        &[
            page("Invoices…", "sales/print-invoices"),
            form("Sales Receipts…", "sales/print-forms", "sales-receipts"),
            form("Credit Memos…", "sales/print-forms", "credit-memos"),
            Entry::Sep,
            page("Checks…", "expenses/print-checks"),
            form("Bill Payment Stubs…", "sales/print-forms", "bill-payments"),
            form("Purchase Orders…", "sales/print-forms", "purchase-orders"),
            Entry::Sep,
            page("Statements…", "sales/statements"),
            page("Mailing Labels…", "sales/mailing-labels"),
        ],
    ),
    page("Send Forms…", "sales/send-forms"),
    // Prints what is showing: the open form, else a register's rows for its dates (printCommand.js),
    // else the window. No accelerator: the page handles Ctrl+P itself (formKeys.js), as for Edit.
    Entry::Cmd { label: "Print… (Ctrl+P)", command: "print", accel: None },
    Entry::Sep,
];

/// Acts on the transaction form open in the focused window (`runFormCommand` in formFlow.jsx).
/// No accelerators: the page handles Ctrl+D, Ctrl+M, Ctrl+G and the line keys itself, so the keys
/// work while typing in a field, and Cmd+M stays Minimize on macOS.
const fn form_cmd(label: &'static str, command: &'static str) -> Entry {
    Entry::Cmd { label, command, accel: None }
}

/// Added to the end of Edit.
const EDIT: &[Entry] = &[
    Entry::Sep,
    form_cmd("Delete or Void Transaction (Ctrl+D)", "form:delete"),
    form_cmd("Memorize Transaction (Ctrl+M)", "form:memorize"),
    form_cmd("Insert Line (Ctrl+Insert)", "form:insert-line"),
    form_cmd("Delete Line (Ctrl+Delete)", "form:delete-line"),
    form_cmd("Copy Line (Ctrl+Alt+Y)", "form:copy-line"),
    form_cmd("Paste Line (Ctrl+Alt+V)", "form:paste-line"),
    form_cmd("Go To Transfer (Ctrl+G)", "form:goto-transfer"),
    form_cmd("Duplicate Transaction (Ctrl+O)", "form:duplicate"),
    Entry::Sep,
    // Act on the customer, vendor or item list on screen (`runListCommand` in listCommands.js).
    form_cmd("New in List", "list:new"),
    form_cmd("Edit in List", "list:edit"),
    form_cmd("Delete from List", "list:delete"),
    form_cmd("Make Inactive / Active", "list:toggle-active"),
    form_cmd("Show All (inactive too)", "list:show-all"),
    Entry::Sep,
    page("Use Register", "accounting/register"),
    keyed("Find…", "accounting/find", None, "CmdOrCtrl+F"),
    Entry::Sep,
    form("Preferences…", "settings", "preferences"),
];

/// Larger and Smaller Text change the active window's kind (lists, registers, forms or reports); their
/// keys, Cmd/Ctrl with + and −, are handled by the page (App.jsx), so typing them in a box still works
/// the same on every platform.
const VIEW: &[Entry] = &[
    Entry::Cmd { label: "Show or Hide Left Icon Bar", command: "sidebar", accel: None },
    Entry::Sep,
    Entry::Cmd { label: "Larger Text", command: "text-larger", accel: None },
    Entry::Cmd { label: "Smaller Text", command: "text-smaller", accel: None },
    Entry::Cmd { label: "Actual Text Size", command: "text-actual", accel: None },
];

const LISTS: &[Entry] = &[
    CHART_OF_ACCOUNTS,
    ITEM_LIST,
    page("Sales Tax Code List", "sales/tax"),
    page("Class List", "accounting/classes"),
    Entry::Sep,
    page("Customer List", "sales/customers"),
    page("Vendor List", "expenses/vendors"),
    Entry::Sub(
        "Customer & Vendor Profile Lists",
        &[
            page("Sales Rep List", "sales/lists"),
            form("Customer Type List", "sales/profile-lists", "customer-types"),
            form("Vendor Type List", "sales/profile-lists", "vendor-types"),
            form("Job Type List", "sales/profile-lists", "job-types"),
            form("Terms List", "sales/profile-lists", "terms"),
            form("Customer Message List", "sales/profile-lists", "customer-messages"),
            form("Payment Method List", "sales/profile-lists", "payment-methods"),
            page("Ship Via List", "sales/lists"),
        ],
    ),
    form("Price Level List", "sales/products", "price-levels"),
    form("Vehicle List", "employees/mileage", "vehicles"),
    // Preferences › Multiple Currencies: the home currency, symbol and exchange-rate lookup.
    form("Currency List", "settings", "currencies"),
    Entry::Sep,
    form("Templates", "settings", "forms"),
    keyed("Memorized Transaction List", "accounting/recurring", None, "CmdOrCtrl+T"),
];

const COMPANY: &[Entry] = &[
    page("Home Page", "dashboard"),
    page("To Do List", "accounting/todos"),
    page("Reminders", "dashboard"),
    Entry::Sep,
    form("Set Up Users and Passwords…", "settings", "users"),
    form("Set Closing Date…", "settings", "closing"),
    page("Email Setup…", "settings/email"),
    page("AI Access (MCP)…", "settings/ai"),
    page("Bank Feeds", "banking/feeds"),
    Entry::Sub("Planning & Budgeting", &[page("Set Up Budgets", "accounting/budgets")]),
    Entry::Sep,
    CHART_OF_ACCOUNTS,
    form("Make General Journal Entries…", "accounting/journal", "journal-entry"),
    page("Reclassify Transactions…", "accounting/reclassify"),
    page("Clean Up Books…", "accounting/clean-up"),
    page("Audit Log", "accounting/audit"),
    page("Attachments Library", "accounting/attachments"),
    Entry::Sub("Manage Currency", &[page("Home Currency Adjustment…", "accounting/fx-revaluation")]),
];

const CUSTOMERS: &[Entry] = &[
    keyed("Customer Center", "sales/customers", None, "CmdOrCtrl+J"),
    Entry::Sep,
    form("Create Estimates", "sales/estimates", "estimate"),
    form("Create Sales Orders", "sales/orders", "sales-order"),
    keyed("Create Invoices", "sales/invoices", Some("invoice"), "CmdOrCtrl+I"),
    form("Enter Sales Receipts", "sales/all", "sales-receipt"),
    page("Create Statements…", "sales/statements"),
    page("Assess Finance Charges…", "sales/finance-charges"),
    page("Invoice for Time & Expenses…", "sales/bill-costs"),
    Entry::Sep,
    form("Receive Payments", "sales/invoices", "receive-payment"),
    form("Create Credit Memos/Refunds", "sales/credits", "credit-memo"),
    Entry::Sep,
    ITEM_LIST,
    form("Change Item Prices", "sales/products", "change-prices"),
];

const VENDORS: &[Entry] = &[
    page("Vendor Center", "expenses/vendors"),
    Entry::Sep,
    form("Enter Bills", "expenses/bills", "bill"),
    // Receipts and bills waiting to be entered (the document inbox).
    page("Document Inbox", "expenses/inbox"),
    form("Pay Bills", "expenses/bills", "pay-bills"),
    form("Enter Vendor Credits", "expenses/credits", "vendor-credit"),
    Entry::Sub("Sales Tax", &[page("Manage Sales Tax…", "sales/tax"), form("Pay Sales Tax…", "sales/tax", "pay-sales-tax"), report("Sales Tax Liability", "sales-tax")]),
    Entry::Sep,
    form("Create Purchase Orders", "expenses/purchase-orders", "purchase-order"),
    page("Receive Items", "expenses/item-receipts"),
    form("Enter Bill for Received Items", "expenses/item-receipts", "bill-received"),
    Entry::Sep,
    Entry::Sub("Print/E-file 1099s", &[report("1099 Summary", "1099"), report("1099-NEC and 1096", "1099-nec")]),
    ITEM_LIST,
];

const EMPLOYEES: &[Entry] = &[
    Entry::Sub(
        "Enter Time",
        &[form("Use Weekly Timesheet", "employees/time", "weekly-timesheet"), form("Time / Enter Single Activity", "employees/time", "time-activity")],
    ),
    form("Enter Vehicle Mileage", "employees/mileage", "mileage-trip"),
];

const BANKING: &[Entry] = &[
    // Ctrl+W, as in the classic software; on macOS Cmd+W closes the window.
    Entry::Go { label: "Write Checks", view: "expenses/expenses", action: Some("check"), report: None, accel: if cfg!(target_os = "macos") { None } else { Some("Ctrl+W") } },
    form("Enter Credit Card Charges", "expenses/expenses", "expense"),
    keyed("Use Register", "accounting/register", None, "CmdOrCtrl+R"),
    Entry::Sep,
    form("Make Deposits", "banking/transfers", "bank-deposit"),
    form("Transfer Funds", "banking/transfers", "transfer"),
    form("Pay Down Credit Card…", "banking/transfers", "pay-card"),
    page("Record Bounced Check…", "banking/bounced-checks"),
    page("Reconcile", "banking/reconcile"),
    Entry::Sep,
    Entry::Sub("Bank Feeds", &[page("Bank Feeds Center", "banking/feeds")]),
];

const REPORTS: &[Entry] = &[
    page("Report Center", "reports"),
    // Memorized reports are listed in the Report Center, which the first item opens.
    Entry::Sub("Memorized Reports", &[page("Memorized Report List", "reports"), report("Process Multiple Reports…", "process-multiple")]),
    // Act on the transaction open on a form (its toolbar's History and Journal, formFlow.jsx).
    // Acts on the account, name or item selected in the list or register in front (its Ctrl+Q).
    form_cmd("QuickReport (Ctrl+Q)", "quick-report"),
    form_cmd("Transaction History (Ctrl+H)", "form:history"),
    form_cmd("Transaction Journal (Ctrl+Y)", "form:journal"),
    Entry::Sep,
    Entry::Sub(
        "Company & Financial",
        &[
            report("Company Snapshot", "snapshot"),
            Entry::Sep,
            report("Profit & Loss Standard", "pl"),
            report("Profit & Loss Detail", "preset:pl-detail"),
            report("Profit & Loss YTD Comparison", "list:pl-ytd"),
            report("Profit & Loss Prev Year Comparison", "pl-py"),
            report("Profit & Loss by Month", "preset:pl-by-month"),
            report("Profit & Loss by Class", "plc"),
            Entry::Sep,
            report("Income by Customer Summary", "preset:income-customer-summary"),
            report("Income by Customer Detail", "preset:income-customer-detail"),
            report("Expenses by Vendor Summary", "expenses-vendor"),
            report("Expenses by Vendor Detail", "preset:expenses-vendor-detail"),
            report("Expenses by Vendor by Month", "preset:expenses-vendor-month"),
            Entry::Sep,
            report("Balance Sheet Standard", "bs"),
            report("Balance Sheet Detail", "preset:bs-detail"),
            report("Balance Sheet Summary", "list:bs-summary"),
            report("Balance Sheet Prev Year Comparison", "bs-py"),
            Entry::Sep,
            report("Statement of Cash Flows", "cf"),
            report("Cash Flow Forecast", "cash-forecast"),
            report("Net Income Detail", "preset:net-income-detail"),
            Entry::Sep,
            report("Income & Expense Graph", "graph-ie"),
            report("Net Worth Graph", "graph-nw"),
        ],
    ),
    Entry::Sub(
        "Customers & Receivables",
        &[
            report("A/R Aging Summary", "ar"),
            report("A/R Aging Detail", "preset:ar-aging-detail"),
            Entry::Sep,
            report("Customer Balance Summary", "preset:customer-balance-summary"),
            report("Customer Balance Detail", "preset:customer-balance-detail"),
            report("Open Invoices", "preset:open-invoices"),
            report("Collections Report", "collections"),
            report("Average Days to Pay Summary", "list:average-days-to-pay"),
            Entry::Sep,
            report("Accounts Receivable Graph", "graph-ar"),
            Entry::Sep,
            report("Transaction List by Customer", "preset:transaction-list-customer"),
            report("Customer Phone List", "list:customer-phone"),
            report("Customer Contact List", "list:customer-contact"),
        ],
    ),
    Entry::Sub(
        "Sales",
        &[
            report("Sales by Customer Summary", "sales-customer"),
            report("Sales by Customer Detail", "preset:sales-customer-detail"),
            Entry::Sep,
            report("Sales by Item Summary", "sales-product"),
            report("Sales by Item Detail", "preset:sales-product-detail"),
            Entry::Sep,
            report("Sales by Rep Summary", "sales-rep"),
            report("Sales by Rep Detail", "preset:sales-rep-detail"),
            Entry::Sep,
            report("Item Price List", "item-price-list"),
            report("Sales by Ship To Address", "list:sales-ship-to"),
            Entry::Sep,
            report("Open Sales Orders by Customer", "list:open-so-customer"),
            report("Open Sales Orders by Item", "list:open-so-item"),
            report("Pending Sales", "list:pending-sales"),
            Entry::Sep,
            report("Sales Graph", "graph-sales"),
        ],
    ),
    Entry::Sub(
        "Jobs, Time & Mileage",
        &[
            report("Job Profitability Summary", "job-profit"),
            report("Job Profitability Detail", "job-profit-detail"),
            report("Job Estimates vs. Actuals Summary", "job-est"),
            report("Job Estimates vs. Actuals Detail", "job-est-detail"),
            report("Item Profitability", "item-profit"),
            report("Invoice Profitability by Customer", "list:invoice-profit-customer"),
            report("Invoice Profitability Detail", "list:invoice-profit-detail"),
            report("Item Estimates vs. Actuals", "item-est"),
            report("Profit & Loss by Job", "pl-job"),
            report("Estimates by Job", "est-by-job"),
            report("Progress Invoices vs. Estimates", "progress"),
            Entry::Sep,
            report("Time by Job Summary", "list:time-job-summary"),
            report("Time by Job Detail", "list:time-job-detail"),
            report("Time by Name", "list:time-name"),
            report("Time by Item", "list:time-item"),
            Entry::Sep,
            report("Mileage by Vehicle Summary", "mileage-vehicle"),
            report("Mileage by Vehicle Detail", "mileage-vehicle-detail"),
            report("Mileage by Job Summary", "mileage-job"),
            report("Mileage by Job Detail", "mileage-job-detail"),
        ],
    ),
    Entry::Sub(
        "Vendors & Payables",
        &[
            report("A/P Aging Summary", "ap"),
            report("A/P Aging Detail", "preset:ap-aging-detail"),
            Entry::Sep,
            report("Vendor Balance Summary", "preset:vendor-balance-summary"),
            report("Vendor Balance Detail", "preset:vendor-balance-detail"),
            report("Unpaid Bills Detail", "preset:unpaid-bills"),
            report("Unpaid Bills by Job", "list:unpaid-bills-job"),
            Entry::Sep,
            report("Accounts Payable Graph", "graph-ap"),
            Entry::Sep,
            report("Sales Tax Liability", "sales-tax"),
            report("Sales Tax Revenue Summary", "list:sales-tax-revenue"),
            Entry::Sep,
            report("1099 Summary", "1099"),
            report("1099 Detail", "preset:1099-detail"),
            report("1099-NEC and 1096", "1099-nec"),
            Entry::Sep,
            report("Transaction List by Vendor", "preset:transaction-list-vendor"),
            report("Vendor Phone List", "list:vendor-phone"),
            report("Vendor Contact List", "list:vendor-contact"),
        ],
    ),
    Entry::Sub(
        "Purchases",
        &[
            report("Purchases by Vendor Summary", "preset:purchases-vendor-summary"),
            report("Purchases by Vendor Detail", "preset:purchases-vendor-detail"),
            Entry::Sep,
            report("Purchases by Item Summary", "preset:purchases-item-summary"),
            report("Purchases by Item Detail", "preset:purchases-item-detail"),
            Entry::Sep,
            report("Open Purchase Orders", "open-pos"),
            report("Open Purchase Orders Detail", "list:open-po-detail"),
        ],
    ),
    Entry::Sub(
        "Inventory",
        &[
            report("Inventory Valuation Summary", "inv-val"),
            report("Inventory Valuation Detail", "inv-detail"),
            report("Inventory Stock Status by Item", "stock-status"),
            Entry::Sep,
            report("Physical Inventory Worksheet", "list:physical-inventory"),
            report("Inventory Turnover by Item", "list:inventory-turnover"),
        ],
    ),
    Entry::Sub(
        "Banking",
        &[
            report("Deposit Detail", "preset:deposit-detail"),
            report("Check Detail", "preset:check-detail"),
            report("Missing Checks", "missing-checks"),
            Entry::Sep,
            report("Previous Reconciliation", "prev-rec"),
            report("Reconciliation Discrepancy", "rec-discrepancy"),
        ],
    ),
    Entry::Sub(
        "Accountant & Taxes",
        &[
            report("Adjusting Journal Entries", "preset:adjusting-journal"),
            Entry::Sep,
            report("Trial Balance", "tb"),
            report("General Ledger", "gl"),
            report("Transaction Detail by Account", "preset:transaction-detail-account"),
            report("Journal", "preset:journal"),
            Entry::Sep,
            report("Audit Trail", "list:audit-trail"),
            report("Voided Transactions", "preset:voided-transactions"),
            Entry::Sep,
            report("Transaction List by Date", "preset:transaction-list-date"),
            report("Account Listing", "list:account-listing"),
            Entry::Sep,
            report("Income Tax Summary", "income-tax-summary"),
            report("Income Tax Detail", "income-tax-detail"),
            Entry::Sep,
            report("Memorized Transaction Listing", "list:memorized-transactions"),
            report("Realized Gains & Losses", "list:realized-gains"),
            report("Unrealized Gains & Losses", "list:unrealized-gains"),
            report("Commented Reports", "commented"),
        ],
    ),
    Entry::Sub("Budgets & Forecasts", &[report("Budget Overview", "list:budget-overview"), report("Budget vs. Actual", "bva"), report("Balance Sheet Budget vs. Actual", "bs-bva"), Entry::Sep, report("Budget vs. Actual Graph", "graph-bva")]),
    Entry::Sub("List", &[
        report("Account Listing", "list:account-listing"),
        report("Item Listing", "list:item-listing"),
        report("Item Price List", "item-price-list"),
        Entry::Sep,
        report("Customer Phone List", "list:customer-phone"),
        report("Customer Contact List", "list:customer-contact"),
        report("Vendor Phone List", "list:vendor-phone"),
        report("Vendor Contact List", "list:vendor-contact"),
        Entry::Sep,
        report("To Do Notes", "list:todo-notes"),
    ]),
];

/// Added to Help below LocalBooks Help.
const HELP: &[Entry] = &[page("Check for Updates…", "settings/updates"), page("Save Diagnostics…", "settings/diagnostics")];

/// Menus added between View and Window, with the classic Alt-key mnemonics.
const MENUS: &[(&str, &[Entry])] =
    &[("&Lists", LISTS), ("&Company", COMPANY), ("C&ustomers", CUSTOMERS), ("Vend&ors", VENDORS), ("E&mployees", EMPLOYEES), ("&Banking", BANKING), ("&Reports", REPORTS)];

/// Builds the items of `entries`, numbering navigation ids so a repeated item keeps a unique id.
fn build(app: &AppHandle, entries: &[Entry], n: &mut usize) -> tauri::Result<Vec<MenuItemKind<Wry>>> {
    let mut items = Vec::with_capacity(entries.len());
    for entry in entries {
        items.push(match entry {
            Entry::Go { label, view, action, report, accel } => {
                *n += 1;
                let id = format!("{GO_PREFIX}{view}|{}|{}|{n}", action.unwrap_or(""), report.unwrap_or(""));
                MenuItemKind::MenuItem(MenuItem::with_id(app, id, literal(label), true, *accel)?)
            }
            Entry::Cmd { label, command, accel } => MenuItemKind::MenuItem(MenuItem::with_id(app, format!("{CMD_PREFIX}{command}"), literal(label), true, *accel)?),
            Entry::Sub(label, children) => MenuItemKind::Submenu(submenu(app, &literal(label), children, n)?),
            Entry::Sep => MenuItemKind::Predefined(PredefinedMenuItem::separator(app)?),
        });
    }
    Ok(items)
}

/// A label shown as written: a single `&` marks the next letter as the Alt-key mnemonic (and is
/// dropped on macOS), so a real ampersand is doubled.
fn literal(label: &str) -> String {
    label.replace('&', "&&")
}

/// `label` may carry a mnemonic (`C&ustomers`).
fn submenu(app: &AppHandle, label: &str, entries: &[Entry], n: &mut usize) -> tauri::Result<Submenu<Wry>> {
    let items = build(app, entries, n)?;
    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
    Submenu::with_items(app, label, true, &refs)
}

fn find_submenu(menu: &Menu<Wry>, text: &str) -> tauri::Result<Option<Submenu<Wry>>> {
    Ok(menu.items()?.into_iter().filter_map(|item| item.as_submenu().cloned()).find(|sub| sub.text().is_ok_and(|t| t == text)))
}

fn insert_all(sub: &Submenu<Wry>, items: &[MenuItemKind<Wry>], at: usize) -> tauri::Result<()> {
    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
    sub.insert_items(&refs, at)
}

/// Adds the menus to the platform's default menu bar, after `file_menu::add_company_items` (whose
/// items `FILE`'s follow).
pub fn install(app: &AppHandle, menu: &Menu<Wry>) -> tauri::Result<()> {
    let mut n = 0;
    if let Some(file) = find_submenu(menu, "File")? {
        insert_all(&file, &build(app, FILE, &mut n)?, crate::file_menu::COMPANY_ITEM_COUNT)?;
    }
    if let Some(edit) = find_submenu(menu, "Edit")? {
        let items = build(app, EDIT, &mut n)?;
        insert_all(&edit, &items, edit.items()?.len())?;
    }
    // macOS has a View menu (Enter Full Screen); elsewhere View follows Edit.
    let view = match find_submenu(menu, "View")? {
        Some(view) => {
            insert_all(&view, &build(app, VIEW, &mut n)?, 0)?;
            view
        }
        None => {
            let view = submenu(app, "&View", VIEW, &mut n)?;
            let after_edit = menu.items()?.iter().position(|i| i.as_submenu().is_some_and(|s| s.text().is_ok_and(|t| t == "Edit"))).map_or(0, |i| i + 1);
            menu.insert(&view, after_edit)?;
            view
        }
    };
    let after_view = menu.items()?.iter().position(|i| i.id() == view.id()).map_or(0, |i| i + 1);
    for (at, (label, entries)) in (after_view..).zip(MENUS) {
        menu.insert(&submenu(app, label, entries, &mut n)?, at)?;
    }
    // Windows and Linux: Help opens with the app's own help (F1, which the page handles).
    if let Some(help) = menu.get(HELP_SUBMENU_ID).and_then(|item| item.as_submenu().cloned()) {
        help.insert(&MenuItem::with_id(app, format!("{CMD_PREFIX}help"), "LocalBooks Help", true, None::<&str>)?, 0)?;
        help.insert(&PredefinedMenuItem::separator(app)?, 1)?;
        for (at, item) in (1..).zip(build(app, HELP, &mut n)?) {
            help.insert(&item, at)?;
        }
    }
    Ok(())
}

/// Enables or greys out every item that opens `view`, for a feature the company turned off in
/// Preferences (Customers › Create Estimates when estimates are off). The menu bar is the app's,
/// so it follows the company in the window that changed it last.
pub fn set_view_enabled(app: &AppHandle, view: &str, enabled: bool) -> tauri::Result<()> {
    fn walk(items: Vec<MenuItemKind<Wry>>, prefix: &str, enabled: bool) -> tauri::Result<()> {
        for item in items {
            match &item {
                MenuItemKind::MenuItem(i) if i.id().as_ref().starts_with(prefix) => i.set_enabled(enabled)?,
                MenuItemKind::Submenu(sub) => walk(sub.items()?, prefix, enabled)?,
                _ => {}
            }
        }
        Ok(())
    }
    match app.menu() {
        Some(menu) => walk(menu.items()?, &format!("{GO_PREFIX}{view}|"), enabled),
        None => Ok(()),
    }
}

/// The page a navigation item opens, as `{ view, action, data }`.
fn navigation(id: &str) -> Option<serde_json::Value> {
    let mut parts = id.strip_prefix(GO_PREFIX)?.split('|');
    let (view, action, report) = (parts.next()?, parts.next()?, parts.next()?);
    let action = (!action.is_empty()).then_some(action);
    let data = (!report.is_empty()).then(|| json!({ "report": report }));
    Some(json!({ "view": view, "action": action, "data": data }))
}

/// Sends a menu choice to the focused window; other menu ids are ignored.
pub fn on_menu_event(app: &AppHandle, id: &str) {
    let (event, payload) = if let Some(nav) = navigation(id) {
        (MENU_NAVIGATE, nav)
    } else if let Some(command) = id.strip_prefix(CMD_PREFIX) {
        (MENU_COMMAND, json!(command))
    } else {
        return;
    };
    if let Some(window) = crate::windows::target(app) {
        let _ = window.emit_to(window.label(), event, payload);
        crate::windows::focus_window(&window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_ids_carry_view_action_and_report() {
        assert_eq!(navigation("go:sales/invoices|invoice||3"), Some(json!({ "view": "sales/invoices", "action": "invoice", "data": null })));
        assert_eq!(navigation("go:reports|open|ar|9"), Some(json!({ "view": "reports", "action": "open", "data": { "report": "ar" } })));
        assert_eq!(navigation("go:dashboard|||1"), Some(json!({ "view": "dashboard", "action": null, "data": null })));
        assert_eq!(navigation("file-open-company"), None);
    }

    /// Every item opens a page the app has (`Sidebar.jsx`) and a report the Reports list has.
    #[test]
    fn items_open_pages_and_reports_that_exist() {
        let read = |path: &str| std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/components/").to_string() + path).unwrap();
        let pages = read("Sidebar.jsx");
        let reports = ["Reports.jsx", "JobReports.jsx", "MileageReports.jsx", "AccountantReports.jsx", "Graphs.jsx"].map(read).concat();
        fn walk(entries: &[Entry], out: &mut Vec<(&'static str, Option<&'static str>)>) {
            for entry in entries {
                match entry {
                    Entry::Go { view, report, .. } => out.push((view, *report)),
                    Entry::Sub(_, children) => walk(children, out),
                    _ => {}
                }
            }
        }
        let mut items = Vec::new();
        for entries in [FILE, EDIT, VIEW, HELP].into_iter().chain(MENUS.iter().map(|(_, e)| *e)) {
            walk(entries, &mut items);
        }
        let presets: Vec<&str> = crate::custom_reports::presets().iter().map(|p| p.id).collect();
        let lists: Vec<&str> = crate::list_reports::catalog().iter().map(|r| r.id).collect();
        let process_multiple = read("Reports.jsx").contains("const PROCESS_MULTIPLE = 'process-multiple';");
        for (view, report) in items {
            assert!(pages.contains(&format!("id: '{view}'")), "no page {view}");
            match report.map(|r| r.split_once(':').unwrap_or(("", r))) {
                None => {}
                Some(("preset", id)) => assert!(presets.contains(&id), "no report preset {id}"),
                Some(("list", id)) => assert!(lists.contains(&id), "no list report {id}"),
                Some(("", "process-multiple")) => assert!(process_multiple, "no Process Multiple Reports"),
                Some(("", id)) => assert!(reports.contains(&format!("['{id}', ")), "no report {id}"),
                Some((kind, id)) => panic!("unknown report kind {kind}:{id}"),
            }
        }
    }

    /// The menus' form items (Edit's, and Reports' History and Journal) name commands the page runs
    /// (`runFormCommand` in formFlow.jsx).
    #[test]
    fn form_commands_exist_in_the_page() {
        // The commands are named in formKeys.js (the keys) and formFlow.jsx (what runs them).
        let flow = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/formFlow.jsx")).unwrap()
            + &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/formKeys.js")).unwrap();
        let app = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/App.jsx")).unwrap();
        assert!(app.contains("runFormCommand(payload.slice(5))"), "App.jsx passes form: commands on");
        fn walk(entries: &[Entry], out: &mut Vec<&'static str>) {
            for entry in entries {
                match entry {
                    Entry::Cmd { command, .. } => out.extend(command.strip_prefix("form:")),
                    Entry::Sub(_, children) => walk(children, out),
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        for entries in [FILE, EDIT, VIEW, HELP].into_iter().chain(MENUS.iter().map(|(_, e)| *e)) {
            walk(entries, &mut found);
        }
        for name in &found {
            assert!(flow.contains(&format!("'{name}'")), "formFlow.jsx has no command {name}");
        }
        found.sort_unstable();
        assert_eq!(
            found,
            ["copy-line", "delete", "delete-line", "duplicate", "goto-transfer", "history", "insert-line", "journal", "memorize", "paste-line"]
        );
    }

    /// Every report the Report Center lists (its own, the presets and the list reports) has a
    /// menu entry under Reports, so the menu bar reaches all of them.
    #[test]
    fn every_report_has_a_menu_entry() {
        fn walk(entries: &[Entry], out: &mut Vec<&'static str>) {
            for entry in entries {
                match entry {
                    Entry::Go { report: Some(id), .. } => out.push(id),
                    Entry::Sub(_, children) => walk(children, out),
                    _ => {}
                }
            }
        }
        let mut menu = Vec::new();
        walk(REPORTS, &mut menu);
        for p in crate::custom_reports::presets() {
            assert!(menu.contains(&format!("preset:{}", p.id).as_str()), "Reports menu has no preset {}", p.id);
        }
        for r in crate::list_reports::catalog() {
            assert!(menu.contains(&format!("list:{}", r.id).as_str()), "Reports menu has no list report {}", r.id);
        }
        // The Report Center's own reports, as `['id', ` in its catalogs.
        let mut checked = 0;
        let read = |path: &str| std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/components/").to_string() + path).unwrap();
        for file in ["Reports.jsx", "JobReports.jsx", "MileageReports.jsx", "AccountantReports.jsx", "Graphs.jsx"] {
            let text = read(file);
            for (at, _) in text.match_indices("['") {
                let rest = &text[at + 2..];
                let Some(end) = rest.find("', ") else { continue };
                let id = &rest[..end];
                // Report ids are lower-case words joined by hyphens; other arrays (view switches) are skipped.
                let is_id = !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                if is_id && !NOT_REPORTS.contains(&id) {
                    assert!(menu.contains(&id), "Reports menu has no report {id} (from {file})");
                    checked += 1;
                }
            }
        }
        // The catalogs were read (about fifty reports in 0.13).
        assert!(checked >= 40, "only {checked} reports found in the Report Center's catalogs");
    }

    /// View's commands are handled by the page (App.jsx).
    #[test]
    fn view_menu_commands_exist_in_the_page() {
        let app = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/App.jsx")).unwrap();
        for entry in VIEW {
            if let Entry::Cmd { command, .. } = entry {
                assert!(app.contains(&format!("'{command}'")), "App.jsx doesn't handle the View command {command}");
            }
        }
    }
    /// `['…', ` arrays in the report screens that aren't report ids.
    const NOT_REPORTS: &[&str] = &["chart", "table"];

    #[test]
    fn ampersands_in_labels_are_shown() {
        assert_eq!(literal("Company & Financial"), "Company && Financial");
    }
}
