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
    Entry::Sub("Utilities", &[page("Import / Convert…", "settings/import"), form("Condense Data…", "settings", "data")]),
    Entry::Sep,
    Entry::Sub("Print Forms", &[page("Statements…", "sales/statements")]),
    page("Send Forms…", "sales/send-forms"),
    Entry::Sep,
];

/// Added to the end of Edit.
const EDIT: &[Entry] = &[
    Entry::Sep,
    page("Use Register", "accounting/register"),
    keyed("Find…", "accounting/find", None, "CmdOrCtrl+F"),
    Entry::Sep,
    form("Preferences…", "settings", "preferences"),
];

const VIEW: &[Entry] = &[Entry::Cmd { label: "Show or Hide Left Icon Bar", command: "sidebar", accel: None }];

const LISTS: &[Entry] = &[
    CHART_OF_ACCOUNTS,
    ITEM_LIST,
    page("Sales Tax Code List", "sales/tax"),
    page("Class List", "accounting/classes"),
    Entry::Sep,
    page("Customer List", "sales/customers"),
    page("Vendor List", "expenses/vendors"),
    Entry::Sub("Customer & Vendor Profile Lists", &[page("Sales Rep List", "sales/lists"), page("Ship Via List", "sales/lists")]),
    form("Price Level List", "sales/products", "price-levels"),
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
    page("Bank Feeds", "banking/feeds"),
    Entry::Sub("Planning & Budgeting", &[page("Set Up Budgets", "accounting/budgets")]),
    Entry::Sep,
    CHART_OF_ACCOUNTS,
    form("Make General Journal Entries…", "accounting/journal", "journal-entry"),
    page("Reclassify Transactions…", "accounting/reclassify"),
    page("Clean Up Books…", "accounting/clean-up"),
    Entry::Sub("Manage Currency", &[page("Home Currency Adjustment…", "accounting/fx-revaluation")]),
];

const CUSTOMERS: &[Entry] = &[
    keyed("Customer Center", "sales/customers", None, "CmdOrCtrl+J"),
    Entry::Sep,
    form("Create Estimates", "sales/estimates", "estimate"),
    keyed("Create Invoices", "sales/invoices", Some("invoice"), "CmdOrCtrl+I"),
    form("Enter Sales Receipts", "sales/all", "sales-receipt"),
    page("Create Statements…", "sales/statements"),
    page("Assess Finance Charges…", "sales/finance-charges"),
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
    form("Pay Bills", "expenses/bills", "pay-bills"),
    form("Enter Vendor Credits", "expenses/credits", "vendor-credit"),
    Entry::Sub("Sales Tax", &[page("Manage Sales Tax…", "sales/tax"), report("Sales Tax Liability", "sales-tax")]),
    Entry::Sep,
    form("Create Purchase Orders", "expenses/purchase-orders", "purchase-order"),
    Entry::Sep,
    Entry::Sub("Print/E-file 1099s", &[report("1099 Summary", "1099")]),
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
    page("Reconcile", "banking/reconcile"),
    Entry::Sep,
    Entry::Sub("Bank Feeds", &[page("Bank Feeds Center", "banking/feeds")]),
];

const REPORTS: &[Entry] = &[
    page("Report Center", "reports"),
    Entry::Sep,
    Entry::Sub(
        "Company & Financial",
        &[
            report("Profit & Loss Standard", "pl"),
            report("Profit & Loss by Class", "plc"),
            report("Profit & Loss Prev Year Comparison", "pl-py"),
            Entry::Sep,
            report("Balance Sheet Standard", "bs"),
            report("Balance Sheet Prev Year Comparison", "bs-py"),
            Entry::Sep,
            report("Statement of Cash Flows", "cf"),
            Entry::Sep,
            report("Expenses by Vendor Summary", "expenses-vendor"),
            Entry::Sep,
            report("Income & Expense Graph", "graph-ie"),
            report("Net Worth Graph", "graph-nw"),
        ],
    ),
    Entry::Sub("Customers & Receivables", &[report("A/R Aging Summary", "ar"), report("Collections Report", "collections"), Entry::Sep, report("Accounts Receivable Graph", "graph-ar")]),
    Entry::Sub("Sales", &[report("Sales by Customer Summary", "sales-customer"), report("Sales by Item Summary", "sales-product"), report("Sales by Rep Summary", "sales-rep"), report("Item Price List", "item-price-list"), Entry::Sep, report("Sales Graph", "graph-sales")]),
    Entry::Sub(
        "Jobs, Time & Mileage",
        &[
            report("Job Profitability Summary", "job-profit"),
            report("Job Profitability Detail", "job-profit-detail"),
            report("Job Estimates vs. Actuals Summary", "job-est"),
            report("Job Estimates vs. Actuals Detail", "job-est-detail"),
            report("Item Profitability", "item-profit"),
            report("Item Estimates vs. Actuals", "item-est"),
            report("Profit & Loss by Job", "pl-job"),
            report("Estimates by Job", "est-by-job"),
            Entry::Sep,
            report("Mileage by Vehicle Summary", "mileage-vehicle"),
            report("Mileage by Vehicle Detail", "mileage-vehicle-detail"),
            report("Mileage by Job Summary", "mileage-job"),
            report("Mileage by Job Detail", "mileage-job-detail"),
        ],
    ),
    Entry::Sub("Vendors & Payables", &[report("A/P Aging Summary", "ap"), Entry::Sep, report("Accounts Payable Graph", "graph-ap"), Entry::Sep, report("Sales Tax Liability", "sales-tax"), Entry::Sep, report("1099 Summary", "1099")]),
    Entry::Sub("Purchases", &[report("Open Purchase Orders", "open-pos")]),
    Entry::Sub(
        "Inventory",
        &[report("Inventory Valuation Summary", "inv-val"), report("Inventory Valuation Detail", "inv-detail"), report("Inventory Stock Status by Item", "stock-status")],
    ),
    Entry::Sub(
        "Banking",
        &[report("Missing Checks", "missing-checks"), Entry::Sep, report("Previous Reconciliation", "prev-rec"), report("Reconciliation Discrepancy", "rec-discrepancy")],
    ),
    Entry::Sub(
        "Accountant & Taxes",
        &[
            report("Trial Balance", "tb"),
            report("General Ledger", "gl"),
            Entry::Sep,
            report("Income Tax Summary", "income-tax-summary"),
            report("Income Tax Detail", "income-tax-detail"),
            Entry::Sep,
            page("Audit Trail", "accounting/audit"),
        ],
    ),
    Entry::Sub("Budgets & Forecasts", &[report("Budget vs. Actual", "bva"), report("Balance Sheet Budget vs. Actual", "bs-bva"), Entry::Sep, report("Budget vs. Actual Graph", "graph-bva")]),
];

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
    }
    Ok(())
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
        for entries in [FILE, EDIT, VIEW].into_iter().chain(MENUS.iter().map(|(_, e)| *e)) {
            walk(entries, &mut items);
        }
        for (view, report) in items {
            assert!(pages.contains(&format!("id: '{view}'")), "no page {view}");
            if let Some(report) = report {
                assert!(reports.contains(&format!("['{report}', ")), "no report {report}");
            }
        }
    }

    #[test]
    fn ampersands_in_labels_are_shown() {
        assert_eq!(literal("Company & Financial"), "Company && Financial");
    }
}
