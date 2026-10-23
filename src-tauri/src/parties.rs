//! Customer, vendor and product records: the details behind a name, editing, and inactivating.
//!
//! Renaming a customer or vendor renames it everywhere it is shown. Posted documents keep the id,
//! and their printed copies keep the name they were issued under. An inactive customer, vendor or
//! product is hidden from pickers; its history and open balances are unaffected.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::sales::{ProductInput, ProductView};

/// `customer` or `vendor`.
fn table(kind: &str) -> Result<&'static str, String> {
    match kind {
        "customer" => Ok("customers"),
        "vendor" => Ok("vendors"),
        other => Err(format!("PARTY_BAD_KIND: '{other}' is not a customer or vendor")),
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PartyDetails {
    pub id: i64,
    pub name: String,
    pub company_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    /// Billing address (customers) or mailing address (vendors).
    pub address: Option<String>,
    pub terms_days: Option<i64>,
    pub notes: Option<String>,
    /// Vendors only.
    pub tax_id: Option<String>,
    pub is_1099: bool,
    pub is_active: bool,
    /// Customers only: default sales tax rate, or exempt from sales tax.
    pub tax_rate_id: Option<i64>,
    pub is_tax_exempt: bool,
    pub tax_exemption: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartyDetailsInput {
    pub name: String,
    #[serde(default)]
    pub company_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub terms_days: Option<i64>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub tax_id: Option<String>,
    #[serde(default)]
    pub is_1099: bool,
    #[serde(default)]
    pub tax_rate_id: Option<i64>,
    #[serde(default)]
    pub is_tax_exempt: bool,
    #[serde(default)]
    pub tax_exemption: Option<String>,
}

fn clean(text: &Option<String>) -> Option<String> {
    text.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

pub async fn get_party(pool: &SqlitePool, kind: &str, id: i64) -> Result<PartyDetails, String> {
    let sql = match table(kind)? {
        "customers" => "SELECT id, name, company_name, email, phone, billing_address AS address, terms_days, notes,
                               NULL AS tax_id, 0 AS is_1099, is_active, tax_rate_id, is_tax_exempt, tax_exemption FROM customers WHERE id = ?1",
        _ => "SELECT id, name, company_name, email, phone, address, terms_days, notes, tax_id, is_1099, is_active,
                     NULL AS tax_rate_id, 0 AS is_tax_exempt, NULL AS tax_exemption FROM vendors WHERE id = ?1",
    };
    sqlx::query_as::<_, PartyDetails>(sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("PARTY_NOT_FOUND: that {kind} does not exist"))
}

pub async fn update_party(pool: &SqlitePool, kind: &str, id: i64, input: PartyDetailsInput) -> Result<PartyDetails, String> {
    let t = table(kind)?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err("PARTY_NO_NAME: enter a name".into());
    }
    let email = clean(&input.email);
    if let Some(e) = &email {
        if !(e.contains('@') && e.rsplit('@').next().is_some_and(|d| d.contains('.'))) {
            return Err("PARTY_BAD_EMAIL: enter a valid email address".into());
        }
    }
    if input.terms_days.is_some_and(|d| !(0..=365).contains(&d)) {
        return Err("PARTY_BAD_TERMS: terms are 0 to 365 days".into());
    }
    let query = if t == "customers" {
        sqlx::query(
            "UPDATE customers SET name = ?1, company_name = ?2, email = ?3, phone = ?4, billing_address = ?5, terms_days = ?6, notes = ?7,
                    tax_rate_id = ?8, is_tax_exempt = ?9, tax_exemption = ?10 WHERE id = ?11",
        )
        .bind(name)
        .bind(clean(&input.company_name))
        .bind(email)
        .bind(clean(&input.phone))
        .bind(clean(&input.address))
        .bind(input.terms_days)
        .bind(clean(&input.notes))
        .bind(input.tax_rate_id)
        .bind(input.is_tax_exempt)
        .bind(clean(&input.tax_exemption))
        .bind(id)
    } else {
        sqlx::query(
            "UPDATE vendors SET name = ?1, company_name = ?2, email = ?3, phone = ?4, address = ?5, terms_days = ?6, notes = ?7,
                    tax_id = ?8, is_1099 = ?9 WHERE id = ?10",
        )
        .bind(name)
        .bind(clean(&input.company_name))
        .bind(email)
        .bind(clean(&input.phone))
        .bind(clean(&input.address))
        .bind(input.terms_days)
        .bind(clean(&input.notes))
        .bind(clean(&input.tax_id))
        .bind(input.is_1099)
        .bind(id)
    };
    let done = query.execute(pool).await.map_err(|e| {
        let msg = db_err(e);
        if msg.contains("UNIQUE") { format!("PARTY_DUPLICATE: there is already a {kind} named '{name}'") } else { msg }
    })?;
    if done.rows_affected() == 0 {
        return Err(format!("PARTY_NOT_FOUND: that {kind} does not exist"));
    }
    get_party(pool, kind, id).await
}

pub async fn set_party_active(pool: &SqlitePool, kind: &str, id: i64, active: bool) -> Result<PartyDetails, String> {
    let t = table(kind)?;
    sqlx::query(&format!("UPDATE {t} SET is_active = ?1 WHERE id = ?2")).bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_party(pool, kind, id).await
}

/// Changes a product's name, description, default rate and income account. Documents already
/// posted keep what they were posted with.
pub async fn update_product(pool: &SqlitePool, id: i64, input: ProductInput) -> Result<ProductView, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("PRODUCT_NO_NAME: enter a product or service name".into());
    }
    if input.rate_cents < 0 {
        return Err("PRODUCT_BAD_RATE: the rate cannot be negative".into());
    }
    let income: Option<String> = sqlx::query_scalar(
        "SELECT t.classification FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
    )
    .bind(input.income_account_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    if income.as_deref() != Some("INCOME") {
        return Err("PRODUCT_NOT_INCOME_ACCOUNT: products and services must post to an Income account".into());
    }
    let done = sqlx::query("UPDATE products_services SET name = ?1, description = ?2, rate_cents = ?3, income_account_id = ?4, is_taxable = ?6 WHERE id = ?5")
        .bind(name)
        .bind(input.description.as_deref().map(str::trim).filter(|d| !d.is_empty()))
        .bind(input.rate_cents)
        .bind(input.income_account_id)
        .bind(id)
        .bind(input.is_taxable)
        .execute(pool)
        .await
        .map_err(|e| {
            let msg = db_err(e);
            if msg.contains("UNIQUE") { format!("PRODUCT_DUPLICATE: there is already a product named '{name}'") } else { msg }
        })?;
    if done.rows_affected() == 0 {
        return Err("PRODUCT_NOT_FOUND: that product does not exist".into());
    }
    product(pool, id).await
}

pub async fn set_product_active(pool: &SqlitePool, id: i64, active: bool) -> Result<ProductView, String> {
    sqlx::query("UPDATE products_services SET is_active = ?1 WHERE id = ?2").bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    product(pool, id).await
}

async fn product(pool: &SqlitePool, id: i64) -> Result<ProductView, String> {
    crate::sales::list_products(pool).await?.into_iter().find(|p| p.id == id).ok_or_else(|| "PRODUCT_NOT_FOUND: that product does not exist".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[tokio::test]
    async fn customers_vendors_and_products_can_be_edited_and_inactivated() {
        let pool = test_pool().await;
        let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap();
        crate::sales::create_customer(&pool, "Globex", None).await.unwrap();
        let input = |name: &str| PartyDetailsInput {
            name: name.into(),
            company_name: Some(" Acme Corp ".into()),
            email: Some("ap@acme.test".into()),
            phone: None,
            address: Some("1 Main St\nSpringfield".into()),
            terms_days: Some(15),
            notes: None,
            tax_id: None,
            is_1099: false,
            tax_rate_id: None,
            is_tax_exempt: false,
            tax_exemption: None,
        };
        let saved = update_party(&pool, "customer", acme.id, input("Acme Inc")).await.unwrap();
        assert_eq!((saved.name.as_str(), saved.company_name.as_deref(), saved.terms_days), ("Acme Inc", Some("Acme Corp"), Some(15)));
        assert!(update_party(&pool, "customer", acme.id, input("globex")).await.unwrap_err().starts_with("PARTY_DUPLICATE"));
        assert!(!set_party_active(&pool, "customer", acme.id, false).await.unwrap().is_active);

        let staples = crate::expenses::create_vendor(&pool, "Staples", None).await.unwrap();
        let vendor = update_party(&pool, "vendor", staples.id, PartyDetailsInput { tax_id: Some("12-3456789".into()), is_1099: true, ..input("Staples") }).await.unwrap();
        assert!(vendor.is_1099 && vendor.tax_id.as_deref() == Some("12-3456789"));

        let sales: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4000'").fetch_one(&pool).await.unwrap();
        let rent: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '6300'").fetch_one(&pool).await.unwrap();
        let widget = crate::sales::create_product(&pool, ProductInput { name: "Widget".into(), description: None, rate_cents: 100, income_account_id: sales, is_taxable: true }).await.unwrap();
        let edited = update_product(&pool, widget.id, ProductInput { name: "Widget Pro".into(), description: Some("Better".into()), rate_cents: 250, income_account_id: sales, is_taxable: true }).await.unwrap();
        assert_eq!((edited.name.as_str(), edited.rate_cents), ("Widget Pro", 250));
        let err = update_product(&pool, widget.id, ProductInput { name: "Widget Pro".into(), description: None, rate_cents: 250, income_account_id: rent, is_taxable: true }).await.unwrap_err();
        assert!(err.starts_with("PRODUCT_NOT_INCOME_ACCOUNT"), "{err}");
        assert!(!set_product_active(&pool, widget.id, false).await.unwrap().is_active);
    }
}
