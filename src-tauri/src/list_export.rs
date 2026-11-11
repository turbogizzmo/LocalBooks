//! File › Utilities › Export (T26): the company's lists as an IIF file, and names and addresses as a
//! text file.
//!
//! **Lists to IIF Files** writes the chart of accounts, customers and jobs, vendors, items and
//! classes in the tab-delimited Intuit Interchange Format that LocalBooks' own IIF import reads
//! (`iif_import.rs`), so another company (LocalBooks or the desktop product) can take the same
//! lists. Each list is a `!KEYWORD` header row naming its columns, then one row per record.
//! Sub-accounts, jobs and subclasses are named by their full path ("Parent:Child"). Transactions
//! are not exported.
//!
//! **Addresses to Text File** writes each customer's and vendor's name, company, address, phone and
//! email as tab-separated columns with a header row, for mail merges and labels.
//!
//! Nothing leaves the computer: both are written where the user saves them.

use sqlx::SqlitePool;

use crate::ledger::db_err;

/// An IIF account type for a LocalBooks one (the reverse of `iif_import::account_type`).
fn iif_account_type(account_type: &str) -> &'static str {
    match account_type {
        "BANK" => "BANK",
        "ACCOUNTS_RECEIVABLE" => "AR",
        "OTHER_CURRENT_ASSET" => "OCASSET",
        "FIXED_ASSET" => "FIXASSET",
        "OTHER_ASSET" => "OASSET",
        "ACCOUNTS_PAYABLE" => "AP",
        "CREDIT_CARD" => "CCARD",
        "OTHER_CURRENT_LIABILITY" => "OCLIAB",
        "LONG_TERM_LIABILITY" => "LTLIAB",
        "EQUITY" => "EQUITY",
        "INCOME" => "INC",
        "COST_OF_GOODS_SOLD" => "COGS",
        "OTHER_INCOME" => "EXINC",
        "OTHER_EXPENSE" => "EXEXP",
        _ => "EXP",
    }
}

/// One field as IIF holds it: tabs and line breaks become spaces (a row is one line of tab-separated
/// fields), and a field with a quote in it is quoted with its quotes doubled.
fn field(text: &str) -> String {
    let flat: String = text.chars().map(|c| if matches!(c, '\t' | '\n' | '\r') { ' ' } else { c }).collect();
    let flat = flat.trim().to_string();
    if flat.contains('"') { format!("\"{}\"", flat.replace('"', "\"\"")) } else { flat }
}

fn yes(on: bool) -> &'static str {
    if on {
        "Y"
    } else {
        "N"
    }
}

/// "12.50" for 1250 cents (negative with a leading minus).
fn amount(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", cents.unsigned_abs() / 100, cents.unsigned_abs() % 100)
}

fn row(out: &mut String, cells: &[String]) {
    out.push_str(&cells.join("\t"));
    out.push_str("\r\n");
}

fn header(out: &mut String, cells: &[&str]) {
    out.push_str(&cells.join("\t"));
    out.push_str("\r\n");
}

/// The first five lines of an address, padded, for BADDR1–5 / ADDR1–5.
fn address_lines(address: Option<&str>) -> Vec<String> {
    let mut lines: Vec<String> = address.unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()).map(field).take(5).collect();
    lines.resize(5, String::new());
    lines
}

/// A customer's or vendor's terms as IIF names them: the term's name, else "Net N".
fn terms(name: Option<String>, days: Option<i64>) -> String {
    name.map(|n| field(&n)).or_else(|| days.map(|d| format!("Net {d}"))).unwrap_or_default()
}

/// The company's lists as an IIF file.
pub async fn lists_iif(pool: &SqlitePool) -> Result<String, String> {
    let mut out = String::new();

    type AccountRow = (String, String, Option<String>, Option<String>, bool);
    let accounts: Vec<AccountRow> = sqlx::query_as(
        "SELECT v.full_name, v.account_type, v.description, v.account_number, v.is_active
         FROM v_chart_of_accounts v JOIN chart_of_accounts c ON c.id = v.id
         WHERE c.merged_into_id IS NULL ORDER BY v.full_name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    header(&mut out, &["!ACCNT", "NAME", "ACCNTTYPE", "DESC", "ACCNUM", "HIDDEN"]);
    for (name, kind, description, number, active) in &accounts {
        row(&mut out, &["ACCNT".into(), field(name), iif_account_type(kind).into(), field(description.as_deref().unwrap_or("")), field(number.as_deref().unwrap_or("")), yes(!active).into()]);
    }

    type PartyRow = (String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>, bool, Option<String>, bool);
    let customers: Vec<PartyRow> = sqlx::query_as(
        "WITH RECURSIVE path (id, full_name) AS (
             SELECT id, name FROM customers WHERE parent_id IS NULL
             UNION ALL SELECT c.id, p.full_name || ':' || c.name FROM customers c JOIN path p ON c.parent_id = p.id)
         SELECT p.full_name, c.company_name, c.billing_address, c.phone, c.email, t.name, c.terms_days, c.notes, c.is_active, NULL, 0
         FROM customers c JOIN path p ON p.id = c.id LEFT JOIN terms t ON t.id = c.terms_id
         WHERE c.merged_into_id IS NULL ORDER BY p.full_name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut cust_header = vec!["!CUST", "NAME", "COMPANYNAME"];
    cust_header.extend(["BADDR1", "BADDR2", "BADDR3", "BADDR4", "BADDR5", "PHONE1", "EMAIL", "TERMS", "NOTEPAD", "HIDDEN"]);
    header(&mut out, &cust_header);
    for (name, company, address, phone, email, terms_name, terms_days, notes, active, _, _) in &customers {
        let mut cells = vec!["CUST".into(), field(name), field(company.as_deref().unwrap_or(""))];
        cells.extend(address_lines(address.as_deref()));
        cells.extend([field(phone.as_deref().unwrap_or("")), field(email.as_deref().unwrap_or("")), terms(terms_name.clone(), *terms_days), field(notes.as_deref().unwrap_or("")), yes(!active).into()]);
        row(&mut out, &cells);
    }

    let vendors: Vec<PartyRow> = sqlx::query_as(
        "SELECT v.name, v.company_name, v.address, v.phone, v.email, t.name, v.terms_days, v.notes, v.is_active, v.tax_id, v.is_1099
         FROM vendors v LEFT JOIN terms t ON t.id = v.terms_id
         WHERE v.merged_into_id IS NULL ORDER BY v.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    header(&mut out, &["!VEND", "NAME", "COMPANYNAME", "ADDR1", "ADDR2", "ADDR3", "ADDR4", "ADDR5", "PHONE1", "EMAIL", "TERMS", "NOTEPAD", "HIDDEN", "TAXID", "1099"]);
    for (name, company, address, phone, email, terms_name, terms_days, notes, active, tax_id, is_1099) in &vendors {
        let mut cells = vec!["VEND".into(), field(name), field(company.as_deref().unwrap_or(""))];
        cells.extend(address_lines(address.as_deref()));
        cells.extend([
            field(phone.as_deref().unwrap_or("")),
            field(email.as_deref().unwrap_or("")),
            terms(terms_name.clone(), *terms_days),
            field(notes.as_deref().unwrap_or("")),
            yes(!active).into(),
            field(tax_id.as_deref().unwrap_or("")),
            yes(*is_1099).into(),
        ]);
        row(&mut out, &cells);
    }

    type ItemRow = (String, Option<String>, Option<String>, i64, Option<i64>, String, bool, bool, bool);
    let items: Vec<ItemRow> = sqlx::query_as(
        "SELECT p.name, p.description, a.full_name, p.rate_cents, p.percent_ppm, p.item_type, p.is_taxable, p.is_active,
                EXISTS (SELECT 1 FROM inventory_items i WHERE i.product_id = p.id)
         FROM products_services p LEFT JOIN v_chart_of_accounts a ON a.id = p.income_account_id
         WHERE p.merged_into_id IS NULL ORDER BY p.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    header(&mut out, &["!INVITEM", "NAME", "INVITEMTYPE", "DESC", "ACCNT", "PRICE", "TAXABLE", "HIDDEN"]);
    for (name, description, account, rate, percent, item_type, taxable, active, tracked) in &items {
        let kind = match (item_type.as_str(), tracked) {
            ("OTHER_CHARGE", _) => "OTHC",
            ("DISCOUNT", _) => "DISC",
            (_, true) => "INVENTORY",
            _ => "SERV",
        };
        // A percent charge or discount is priced "10.0%" (ppm: 100,000 = 10.0%).
        let price = match percent {
            Some(ppm) => format!("{}{}.{}%", if *ppm < 0 { "-" } else { "" }, ppm.unsigned_abs() / 10_000, ppm.unsigned_abs() % 10_000 / 1_000),
            None => amount(*rate),
        };
        row(&mut out, &["INVITEM".into(), field(name), kind.into(), field(description.as_deref().unwrap_or("")), field(account.as_deref().unwrap_or("")), price, yes(*taxable).into(), yes(!active).into()]);
    }

    let classes: Vec<(String, bool)> = sqlx::query_as(
        "WITH RECURSIVE path (id, full_name) AS (
             SELECT id, name FROM classes WHERE parent_id IS NULL
             UNION ALL SELECT c.id, p.full_name || ':' || c.name FROM classes c JOIN path p ON c.parent_id = p.id)
         SELECT p.full_name, c.is_active FROM classes c JOIN path p ON p.id = c.id ORDER BY p.full_name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    if !classes.is_empty() {
        header(&mut out, &["!CLASS", "NAME", "HIDDEN"]);
        for (name, active) in &classes {
            row(&mut out, &["CLASS".into(), field(name), yes(!active).into()]);
        }
    }
    Ok(out)
}

/// Every customer's and vendor's name and address as tab-separated text with a header row.
pub async fn addresses_text(pool: &SqlitePool) -> Result<String, String> {
    type PartyRow = (String, String, Option<String>, Option<String>, Option<String>, Option<String>);
    let rows: Vec<PartyRow> = sqlx::query_as(
        "SELECT 'Customer', name, company_name, billing_address, phone, email FROM customers WHERE merged_into_id IS NULL AND is_active = 1
         UNION ALL
         SELECT 'Vendor', name, company_name, address, phone, email FROM vendors WHERE merged_into_id IS NULL AND is_active = 1
         ORDER BY 1, 2 COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = String::new();
    header(&mut out, &["Type", "Name", "Company", "Address 1", "Address 2", "Address 3", "Address 4", "Address 5", "Phone", "Email"]);
    for (kind, name, company, address, phone, email) in &rows {
        let mut cells = vec![kind.clone(), field(name), field(company.as_deref().unwrap_or(""))];
        cells.extend(address_lines(address.as_deref()));
        cells.extend([field(phone.as_deref().unwrap_or("")), field(email.as_deref().unwrap_or(""))]);
        row(&mut out, &cells);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
