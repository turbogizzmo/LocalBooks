//! New Company setup: QuickBooks-style starter charts of accounts, chosen by industry and the
//! income tax form the business files.
//!
//! `company_setup/starter_charts.json` follows QuickBooks' own New Company data: its 33 industries,
//! each industry's **core** accounts (checked by default) and **recommended** accounts (offered,
//! unchecked), and the equity accounts for each tax form, with QuickBooks' five-digit numbers. The
//! descriptions are LocalBooks' own. A company set up this way also gets LocalBooks' system
//! accounts, renumbered QuickBooks-style, and a bank account.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

use crate::ledger::db_err;

#[derive(Debug, Deserialize)]
struct Catalog {
    accounts: Vec<CatalogAccount>,
    industries: Vec<CatalogIndustry>,
    equity: BTreeMap<String, Vec<EquityAccount>>,
}

#[derive(Debug, Deserialize)]
struct CatalogAccount {
    number: String,
    name: String,
    #[serde(rename = "type")]
    account_type: String,
    parent: Option<String>,
    description: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogIndustry {
    name: String,
    sales_tax: bool,
    core: Vec<String>,
    recommended: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EquityAccount {
    number: String,
    name: String,
    description: String,
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| serde_json::from_str(include_str!("company_setup/starter_charts.json")).expect("starter_charts.json is valid"))
}

fn catalog_account(number: &str) -> Option<&'static CatalogAccount> {
    catalog().accounts.iter().find(|a| a.number == number)
}

/// QuickBooks' retained-earnings number; each tax form gives that account its own name.
const RETAINED_EARNINGS_NUMBER: &str = "32000";
const BANK_NUMBER: &str = "10100";

/// The entity types QuickBooks offers as "Income Tax Form Used": (key, label, `tax_form` preference).
/// `None` leaves the preference as it is.
pub const ENTITY_TYPES: &[(&str, &str, Option<&str>)] = &[
    ("sole_proprietorship", "Sole proprietorship (Form 1040)", Some("SCHEDULE_C")),
    ("llc_single", "Single-member LLC (Form 1040)", Some("SCHEDULE_C")),
    ("partnership", "Partnership or LLP (Form 1065)", Some("1065")),
    ("llc_multi", "Multi-member LLC (Form 1065)", Some("1065")),
    ("corporation", "Corporation (Form 1120)", Some("1120")),
    ("s_corporation", "S corporation (Form 1120-S)", Some("1120S")),
    ("nonprofit", "Nonprofit (Form 990)", Some("990")),
    ("other", "Other / none", None),
];

/// System accounts every company has, matched by type and their LocalBooks name, with the number a
/// set-up company gives them.
const SYSTEM_NUMBERS: &[(&str, &str, &str)] = &[
    ("ACCOUNTS_RECEIVABLE", "Accounts Receivable (A/R)", "11000"),
    ("OTHER_CURRENT_ASSET", "Undeposited Funds", "12000"),
    ("ACCOUNTS_PAYABLE", "Accounts Payable (A/P)", "20000"),
    ("EQUITY", "Opening Balance Equity", "30000"),
    ("EQUITY", "Retained Earnings", RETAINED_EARNINGS_NUMBER),
    ("OTHER_EXPENSE", "Exchange Gain or Loss", "80800"),
    ("OTHER_EXPENSE", "Unrealized Exchange Gain or Loss", "80900"),
];

/// Accounts LocalBooks creates when a feature first needs one: (name, type, four-digit number,
/// QuickBooks' five-digit number, description).
pub const INVENTORY_ASSET: StandardAccount = ("Inventory Asset", "OTHER_CURRENT_ASSET", "1400", "12100", "Cost of products on hand for sale");
pub const COST_OF_GOODS_SOLD: StandardAccount = ("Cost of Goods Sold", "COST_OF_GOODS_SOLD", "5000", "50000", "Cost of products sold");
pub const SALES_TAX_PAYABLE: StandardAccount = ("Sales Tax Payable", "OTHER_CURRENT_LIABILITY", "2200", "25500", "Sales tax collected and owed to the tax agency");
pub type StandardAccount = (&'static str, &'static str, &'static str, &'static str, &'static str);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndustryView {
    pub name: String,
    /// Whether QuickBooks turns sales tax on by default for this industry.
    pub sales_tax: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityTypeView {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupOptions {
    pub industries: Vec<IndustryView>,
    pub entity_types: Vec<EntityTypeView>,
}

pub fn options() -> SetupOptions {
    SetupOptions {
        industries: catalog().industries.iter().map(|i| IndustryView { name: i.name.clone(), sales_tax: i.sales_tax }).collect(),
        entity_types: ENTITY_TYPES.iter().map(|(k, l, _)| EntityTypeView { key: k.to_string(), label: l.to_string() }).collect(),
    }
}

/// One line of the New Company account checklist.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StarterAccount {
    pub number: String,
    pub name: String,
    pub account_type: String,
    pub parent_number: Option<String>,
    pub description: String,
    /// "core" (checked by default), "recommended" (offered, unchecked), or "required" (always
    /// created: system, equity and bank accounts; not in the checklist's choices).
    pub group: String,
}

fn industry(name: &str) -> Result<&'static CatalogIndustry, String> {
    catalog().industries.iter().find(|i| i.name == name).ok_or_else(|| format!("SETUP_UNKNOWN_INDUSTRY: '{name}' is not one of the industries"))
}

fn entity(key: &str) -> Result<&'static (&'static str, &'static str, Option<&'static str>), String> {
    ENTITY_TYPES.iter().find(|(k, _, _)| *k == key).ok_or_else(|| format!("SETUP_UNKNOWN_ENTITY: '{key}' is not one of the tax forms"))
}

fn equity_for(entity_key: &str) -> &'static [EquityAccount] {
    catalog().equity.get(entity_key).map(Vec::as_slice).unwrap_or(&[])
}

/// The suggested chart for an industry and tax form: the accounts every set-up company gets, then
/// the industry's core and recommended accounts, in number order.
pub fn starter_chart(industry_name: &str, entity_key: &str) -> Result<Vec<StarterAccount>, String> {
    let ind = industry(industry_name)?;
    entity(entity_key)?;
    let required = |number: &str, name: &str, account_type: &str, description: &str| StarterAccount {
        number: number.into(),
        name: name.into(),
        account_type: account_type.into(),
        parent_number: None,
        description: description.into(),
        group: "required".into(),
    };
    let equity = equity_for(entity_key);
    let retained = equity.iter().find(|e| e.number == RETAINED_EARNINGS_NUMBER);
    let mut out = vec![
        required(BANK_NUMBER, "Checking", "BANK", "Your main business bank account"),
        required("11000", "Accounts Receivable (A/R)", "ACCOUNTS_RECEIVABLE", "Unpaid customer invoices"),
        required("12000", "Undeposited Funds", "OTHER_CURRENT_ASSET", "Customer payments received but not yet deposited"),
        required("20000", "Accounts Payable (A/P)", "ACCOUNTS_PAYABLE", "Unpaid vendor bills"),
        required("30000", "Opening Balance Equity", "EQUITY", "Offset for opening balances entered at setup"),
        required(
            RETAINED_EARNINGS_NUMBER,
            retained.map_or("Retained Earnings", |e| e.name.as_str()),
            "EQUITY",
            retained.map_or("Profits kept in the business from prior years", |e| e.description.as_str()),
        ),
    ];
    out.extend(equity.iter().filter(|e| e.number != RETAINED_EARNINGS_NUMBER).map(|e| required(&e.number, &e.name, "EQUITY", &e.description)));
    for (numbers, group) in [(&ind.core, "core"), (&ind.recommended, "recommended")] {
        for number in numbers {
            let a = catalog_account(number).expect("industry accounts are in the catalog");
            out.push(StarterAccount {
                number: a.number.clone(),
                name: a.name.clone(),
                account_type: a.account_type.clone(),
                parent_number: a.parent.clone(),
                description: a.description.clone(),
                group: group.into(),
            });
        }
    }
    out.sort_by(|a, b| a.number.cmp(&b.number));
    Ok(out)
}

/// The answers from the New Company assistant.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanySetup {
    pub industry: String,
    pub entity_type: String,
    /// The industry accounts chosen in the checklist, by number. A chosen subaccount brings its
    /// parent along.
    #[serde(default)]
    pub accounts: Vec<String>,
    /// The bank account's name; blank means "Checking".
    #[serde(default)]
    pub bank_account_name: Option<String>,
    #[serde(default)]
    pub legal_name: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub ein: Option<String>,
    /// MM-01; omitted keeps January.
    #[serde(default)]
    pub fiscal_year_start: Option<String>,
    #[serde(default)]
    pub sales_tax: bool,
    #[serde(default)]
    pub inventory: bool,
}

/// Replaces a brand-new company's starter chart with the one chosen in the assistant and saves its
/// preferences. Refuses a file that already has transactions.
pub async fn apply(pool: &SqlitePool, setup: &CompanySetup) -> Result<(), String> {
    let ind = industry(&setup.industry)?;
    let (entity_key, _, tax_form) = *entity(&setup.entity_type)?;
    let offered: BTreeSet<&str> = ind.core.iter().chain(&ind.recommended).map(String::as_str).collect();
    let mut chosen = BTreeSet::new();
    for number in &setup.accounts {
        if !offered.contains(number.as_str()) {
            return Err(format!("SETUP_UNKNOWN_ACCOUNT: account {number} is not offered for {}", ind.name));
        }
        let mut next = Some(number.as_str());
        while let Some(n) = next {
            chosen.insert(n);
            next = catalog_account(n).and_then(|a| a.parent.as_deref());
        }
    }

    let mut tx = pool.begin().await.map_err(db_err)?;
    let has_entries: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM journal_entries)").fetch_one(&mut *tx).await.map_err(db_err)?;
    if has_entries {
        return Err("SETUP_NOT_NEW: the starting chart of accounts can only be chosen for a new, empty company".into());
    }
    // The starter accounts the migrations added (newest first, so subaccounts go before parents).
    sqlx::query("DELETE FROM chart_of_accounts WHERE is_system = 0 AND parent_id IS NOT NULL").execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("DELETE FROM chart_of_accounts WHERE is_system = 0").execute(&mut *tx).await.map_err(db_err)?;

    sqlx::query("UPDATE chart_of_accounts SET account_number = NULL WHERE is_system = 1").execute(&mut *tx).await.map_err(db_err)?;
    for (account_type, name, number) in SYSTEM_NUMBERS {
        sqlx::query("UPDATE chart_of_accounts SET account_number = ?1 WHERE is_system = 1 AND account_type = ?2 AND name = ?3")
            .bind(number)
            .bind(account_type)
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    if let Some(retained) = equity_for(entity_key).iter().find(|e| e.number == RETAINED_EARNINGS_NUMBER) {
        sqlx::query("UPDATE chart_of_accounts SET name = ?1, description = ?2 WHERE is_system = 1 AND account_number = ?3")
            .bind(&retained.name)
            .bind(&retained.description)
            .bind(RETAINED_EARNINGS_NUMBER)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }

    let bank = setup.bank_account_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Checking");
    insert(&mut tx, BANK_NUMBER, bank, "BANK", None, Some("Your main business bank account")).await?;
    for e in equity_for(entity_key).iter().filter(|e| e.number != RETAINED_EARNINGS_NUMBER) {
        insert(&mut tx, &e.number, &e.name, "EQUITY", None, Some(&e.description)).await?;
    }
    // Number order puts every parent before its subaccounts.
    for number in &chosen {
        let a = catalog_account(number).expect("offered accounts are in the catalog");
        insert(&mut tx, &a.number, &a.name, &a.account_type, a.parent.as_deref(), Some(&a.description)).await?;
    }
    if setup.inventory {
        ensure_standard_account(&mut tx, INVENTORY_ASSET).await?;
        ensure_standard_account(&mut tx, COST_OF_GOODS_SOLD).await?;
    }
    if setup.sales_tax {
        ensure_standard_account(&mut tx, SALES_TAX_PAYABLE).await?;
    }
    tx.commit().await.map_err(db_err)?;

    let text = |v: &Option<String>| v.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let mut prefs: Vec<(&str, String)> = vec![("sales_tax_enabled", setup.sales_tax.to_string())];
    if let Some(form) = tax_form {
        prefs.push(("tax_form", form.to_string()));
    }
    for (key, value) in [("company_legal_name", &setup.legal_name), ("company_address", &setup.address), ("company_ein", &setup.ein), ("fiscal_year_start", &setup.fiscal_year_start)] {
        if let Some(v) = text(value) {
            prefs.push((key, v));
        }
    }
    for (key, value) in prefs {
        crate::preferences::set(pool, key, &value).await?;
    }
    Ok(())
}

async fn insert(conn: &mut SqliteConnection, number: &str, name: &str, account_type: &str, parent_number: Option<&str>, description: Option<&str>) -> Result<i64, String> {
    sqlx::query_scalar(
        "INSERT INTO chart_of_accounts (account_number, name, account_type, parent_id, description)
         VALUES (?1, ?2, ?3, (SELECT id FROM chart_of_accounts WHERE account_number = ?4), ?5) RETURNING id",
    )
    .bind(number)
    .bind(name)
    .bind(account_type)
    .bind(parent_number)
    .bind(description)
    .fetch_one(conn)
    .await
    .map_err(db_err)
}

/// The id of an active top-level account with this standard name and type, creating it when the
/// company has none. A new account gets QuickBooks' five-digit number when the chart uses five-digit
/// numbers, otherwise the four-digit one, and no number when that one is taken.
pub async fn ensure_standard_account(conn: &mut SqliteConnection, (name, account_type, four, five, description): StandardAccount) -> Result<i64, String> {
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM chart_of_accounts WHERE name = ?1 COLLATE NOCASE AND account_type = ?2 AND parent_id IS NULL AND is_active = 1 ORDER BY id LIMIT 1",
    )
    .bind(name)
    .bind(account_type)
    .fetch_optional(&mut *conn)
    .await
    .map_err(db_err)?;
    if let Some(id) = existing {
        return Ok(id);
    }
    sqlx::query_scalar(
        "INSERT INTO chart_of_accounts (account_number, name, account_type, description)
         SELECT CASE WHEN EXISTS (SELECT 1 FROM chart_of_accounts WHERE account_number = n.number) THEN NULL ELSE n.number END, ?1, ?2, ?3
         FROM (SELECT CASE WHEN EXISTS (SELECT 1 FROM chart_of_accounts WHERE length(account_number) = 5 AND account_number GLOB '[0-9]*')
                           THEN ?5 ELSE ?4 END AS number) n
         RETURNING id",
    )
    .bind(name)
    .bind(account_type)
    .bind(description)
    .bind(four)
    .bind(five)
    .fetch_one(&mut *conn)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests;
