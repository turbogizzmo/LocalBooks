//! Customer, vendor and product records: the details behind a name, editing, and inactivating.
//!
//! Renaming a customer or vendor renames it everywhere it is shown. Posted documents keep the id,
//! and their printed copies keep the name they were issued under. An inactive customer, vendor or
//! product is hidden from pickers; its history and open balances are unaffected.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::sales::{ProductInput, ProductView};
use crate::tax_ids::{self, Access};

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
    /// Vendors only: the full tax ID, when this user may see it.
    pub tax_id: Option<String>,
    /// Vendors only: the tax ID with all but the last four digits starred, whenever there is one.
    pub tax_id_masked: Option<String>,
    /// This user can neither see nor change the tax ID (users are on and they lack sensitive access).
    #[sqlx(default)]
    pub tax_id_locked: bool,
    pub is_1099: bool,
    pub is_active: bool,
    /// Customers only: default sales tax rate, or exempt from sales tax.
    pub tax_rate_id: Option<i64>,
    pub is_tax_exempt: bool,
    pub tax_exemption: Option<String>,
    /// Customers only: the form style their forms use (`None` = the default style).
    pub form_style_id: Option<i64>,
    /// Customers only: the rep filled in on their new sales forms.
    pub sales_rep_id: Option<i64>,
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
    /// Leave the tax ID as it is (the form did not change it, or this user cannot see it).
    #[serde(default)]
    pub keep_tax_id: bool,
    #[serde(default)]
    pub is_1099: bool,
    #[serde(default)]
    pub tax_rate_id: Option<i64>,
    #[serde(default)]
    pub is_tax_exempt: bool,
    #[serde(default)]
    pub tax_exemption: Option<String>,
    #[serde(default)]
    pub form_style_id: Option<i64>,
    #[serde(default)]
    pub sales_rep_id: Option<i64>,
    /// Custom field values to set (fields left out keep theirs).
    #[serde(default)]
    pub custom_fields: Vec<crate::custom_fields::FieldValue>,
}

fn clean(text: &Option<String>) -> Option<String> {
    text.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// A customer or vendor; a vendor's tax ID as `access` may see it.
pub async fn get_party(pool: &SqlitePool, kind: &str, id: i64, access: Access<'_>) -> Result<PartyDetails, String> {
    let sql = match table(kind)? {
        "customers" => "SELECT id, name, company_name, email, phone, billing_address AS address, terms_days, notes,
                               NULL AS tax_id, NULL AS tax_id_masked, 0 AS is_1099, is_active, tax_rate_id, is_tax_exempt, tax_exemption, form_style_id,
                               sales_rep_id
                        FROM customers WHERE id = ?1",
        _ => "SELECT id, name, company_name, email, phone, address, terms_days, notes, tax_id, tax_id_masked, is_1099, is_active,
                     NULL AS tax_rate_id, 0 AS is_tax_exempt, NULL AS tax_exemption, NULL AS form_style_id, NULL AS sales_rep_id
              FROM vendors WHERE id = ?1",
    };
    let mut party = sqlx::query_as::<_, PartyDetails>(sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("PARTY_NOT_FOUND: that {kind} does not exist"))?;
    let view = tax_ids::show(access, id, party.tax_id.as_deref(), party.tax_id_masked.as_deref())?;
    (party.tax_id, party.tax_id_masked) = (view.full, view.masked);
    party.tax_id_locked = !access.can_see();
    Ok(party)
}

pub async fn update_party(pool: &SqlitePool, kind: &str, id: i64, input: PartyDetailsInput, access: Access<'_>) -> Result<PartyDetails, String> {
    let t = table(kind)?;
    let typed = input.name.trim();
    if typed.is_empty() {
        return Err("PARTY_NO_NAME: enter a name".into());
    }
    // Customers may be jobs ("Customer:Job"); renaming one renames its jobs too.
    let full = if t == "customers" { crate::sales::job_name(typed)? } else { typed.to_string() };
    let name = full.as_str();
    let old_name: Option<String> = sqlx::query_scalar(&format!("SELECT name FROM {t} WHERE id = ?1")).bind(id).fetch_optional(pool).await.map_err(db_err)?;
    let old_name = old_name.ok_or_else(|| format!("PARTY_NOT_FOUND: that {kind} does not exist"))?;
    let email = clean(&input.email);
    if let Some(e) = &email {
        if !(e.contains('@') && e.rsplit('@').next().is_some_and(|d| d.contains('.'))) {
            return Err("PARTY_BAD_EMAIL: enter a valid email address".into());
        }
    }
    if input.terms_days.is_some_and(|d| !(0..=365).contains(&d)) {
        return Err("PARTY_BAD_TERMS: terms are 0 to 365 days".into());
    }
    if let Some(style) = input.form_style_id.filter(|_| t == "customers") {
        let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM form_styles WHERE id = ?1").bind(style).fetch_optional(pool).await.map_err(db_err)?;
        if exists.is_none() {
            return Err("PARTY_BAD_FORM_STYLE: that form style no longer exists".into());
        }
    }
    if let Some(rep) = input.sales_rep_id.filter(|_| t == "customers") {
        let rep: Option<(String, bool)> = sqlx::query_as("SELECT initials, is_active FROM sales_reps WHERE id = ?1").bind(rep).fetch_optional(pool).await.map_err(db_err)?;
        let unchanged = sqlx::query_scalar::<_, Option<i64>>("SELECT sales_rep_id FROM customers WHERE id = ?1").bind(id).fetch_optional(pool).await.map_err(db_err)?.flatten()
            == input.sales_rep_id;
        match rep {
            None => return Err("PARTY_BAD_REP: that sales rep no longer exists".into()),
            Some((initials, false)) if !unchanged => return Err(format!("PARTY_INACTIVE_REP: sales rep {initials} is inactive")),
            _ => {}
        }
    }
    let keep_tax_id = input.keep_tax_id || t == "customers";
    if !keep_tax_id && !access.can_see() {
        return Err("TAX_ID_LOCKED: only users who can see sensitive data can change a tax ID".into());
    }
    let (tax_id, tax_id_masked) = if keep_tax_id { (None, None) } else { tax_ids::store(access, id, input.tax_id.as_deref())? };
    let query = if t == "customers" {
        sqlx::query(
            "UPDATE customers SET name = ?1, company_name = ?2, email = ?3, phone = ?4, billing_address = ?5, terms_days = ?6, notes = ?7,
                    tax_rate_id = ?8, is_tax_exempt = ?9, tax_exemption = ?10, form_style_id = ?11, sales_rep_id = ?13 WHERE id = ?12",
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
        .bind(input.form_style_id)
        .bind(id)
        .bind(input.sales_rep_id)
    } else {
        sqlx::query(
            "UPDATE vendors SET name = ?1, company_name = ?2, email = ?3, phone = ?4, address = ?5, terms_days = ?6, notes = ?7,
                    tax_id = CASE WHEN ?11 THEN tax_id ELSE ?8 END, tax_id_masked = CASE WHEN ?11 THEN tax_id_masked ELSE ?12 END,
                    is_1099 = ?9 WHERE id = ?10",
        )
        .bind(name)
        .bind(clean(&input.company_name))
        .bind(email)
        .bind(clean(&input.phone))
        .bind(clean(&input.address))
        .bind(input.terms_days)
        .bind(clean(&input.notes))
        .bind(tax_id)
        .bind(input.is_1099)
        .bind(id)
        .bind(keep_tax_id)
        .bind(tax_id_masked)
    };
    let done = query.execute(pool).await.map_err(|e| {
        let msg = db_err(e);
        if msg.contains("UNIQUE") { format!("PARTY_DUPLICATE: there is already a {kind} named '{name}'") } else { msg }
    })?;
    if done.rows_affected() == 0 {
        return Err(format!("PARTY_NOT_FOUND: that {kind} does not exist"));
    }
    if !input.custom_fields.is_empty() {
        let list = if t == "customers" { crate::custom_fields::FieldList::Customer } else { crate::custom_fields::FieldList::Vendor };
        crate::custom_fields::save_record_values(pool, list, id, &input.custom_fields).await?;
    }
    if t == "customers" && old_name != name {
        let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
        let result = async {
            // The jobs move with their customer...
            sqlx::query("UPDATE customers SET name = ?1 || substr(name, length(?2) + 1) WHERE substr(name, 1, length(?2) + 1) = ?2 || ':'")
                .bind(name)
                .bind(&old_name)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            // ...and a job renamed under another customer moves to it.
            let parent = match name.rsplit_once(':') {
                Some((above, _)) => Some(crate::sales::find_or_create_named(&mut tx, crate::sales::Party::Customer, above).await?),
                None => None,
            };
            if parent == Some(id) {
                return Err("PARTY_BAD_JOB: a customer cannot be its own job".to_string());
            }
            sqlx::query("UPDATE customers SET parent_id = ?1 WHERE id = ?2").bind(parent).bind(id).execute(&mut *tx).await.map_err(db_err)?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err)?,
            Err(err) => return Err(crate::ledger::rollback(tx, err).await),
        }
    }
    get_party(pool, kind, id, access).await
}

pub async fn set_party_active(pool: &SqlitePool, kind: &str, id: i64, active: bool, access: Access<'_>) -> Result<PartyDetails, String> {
    let t = table(kind)?;
    sqlx::query(&format!("UPDATE {t} SET is_active = ?1 WHERE id = ?2")).bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_party(pool, kind, id, access).await
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
mod tests;
