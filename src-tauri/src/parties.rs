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
    /// The default term from the Terms list; `terms_days` follows it.
    pub terms_id: Option<i64>,
    /// Customers: the customer type, and for a job its job type. Vendors: the vendor type.
    pub customer_type_id: Option<i64>,
    pub job_type_id: Option<i64>,
    pub vendor_type_id: Option<i64>,
    /// Customers: this record is a job (it has a parent customer).
    pub is_job: bool,
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
    /// Customers only: the price level their sales forms fill rates from (`None` = standard prices).
    pub price_level_id: Option<i64>,
    /// Customers only: the second address; `address` is the billing address.
    pub shipping_address: Option<String>,
    /// Customers only: the main and second contact, and more ways to reach them.
    pub contact_name: Option<String>,
    pub alt_contact_name: Option<String>,
    pub alt_phone: Option<String>,
    pub fax: Option<String>,
    pub website: Option<String>,
    /// Customers only: the number they know us by, or we know them by.
    pub account_number: Option<String>,
    /// Customers only: an invoice that would take them over this warns before saving.
    pub credit_limit_cents: Option<i64>,
    /// Customers only: their resale certificate or tax ID, as text.
    pub resale_number: Option<String>,
    /// Customers only: how their forms are sent (`EMAIL`, `PRINT`, `NONE`; `None` = not set).
    pub preferred_delivery: Option<String>,
    /// Jobs: `PENDING`, `AWARDED`, `IN_PROGRESS`, `CLOSED` or `NOT_AWARDED` (`None` = not set).
    pub job_status: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
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
    /// The default term; when given, `terms_days` is set from it.
    #[serde(default)]
    pub terms_id: Option<i64>,
    #[serde(default)]
    pub customer_type_id: Option<i64>,
    #[serde(default)]
    pub job_type_id: Option<i64>,
    #[serde(default)]
    pub vendor_type_id: Option<i64>,
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
    #[serde(default)]
    pub price_level_id: Option<i64>,
    #[serde(default)]
    pub shipping_address: Option<String>,
    #[serde(default)]
    pub contact_name: Option<String>,
    #[serde(default)]
    pub alt_contact_name: Option<String>,
    #[serde(default)]
    pub alt_phone: Option<String>,
    #[serde(default)]
    pub fax: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub account_number: Option<String>,
    #[serde(default)]
    pub credit_limit_cents: Option<i64>,
    #[serde(default)]
    pub resale_number: Option<String>,
    pub preferred_delivery: Option<String>,
    #[serde(default)]
    pub job_status: Option<String>,
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
        "customers" => "SELECT id, name, company_name, email, phone, billing_address AS address, terms_days, terms_id,
                               customer_type_id, job_type_id, NULL AS vendor_type_id, parent_id IS NOT NULL AS is_job, notes,
                               NULL AS tax_id, NULL AS tax_id_masked, 0 AS is_1099, is_active, tax_rate_id, is_tax_exempt, tax_exemption, form_style_id,
                               sales_rep_id, price_level_id, preferred_delivery, job_status, shipping_address, contact_name, alt_contact_name, alt_phone, fax,
                               website, account_number, credit_limit_cents, resale_number
                        FROM customers WHERE id = ?1",
        _ => "SELECT id, name, company_name, email, phone, address, terms_days, terms_id,
                     NULL AS customer_type_id, NULL AS job_type_id, vendor_type_id, 0 AS is_job, notes, tax_id, tax_id_masked, is_1099, is_active,
                     NULL AS tax_rate_id, 0 AS is_tax_exempt, NULL AS tax_exemption, NULL AS form_style_id, NULL AS sales_rep_id, NULL AS price_level_id,
                     NULL AS preferred_delivery, NULL AS job_status,
                     NULL AS shipping_address, NULL AS contact_name, NULL AS alt_contact_name, NULL AS alt_phone, NULL AS fax,
                     NULL AS website, NULL AS account_number, NULL AS credit_limit_cents, NULL AS resale_number
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
    if input.credit_limit_cents.is_some_and(|c| !(0..=crate::ledger::MAX_LINE_CENTS).contains(&c)) {
        return Err("PARTY_BAD_CREDIT_LIMIT: the credit limit must be zero or more".into());
    }
    if let Some(site) = clean(&input.website) {
        if site.chars().any(char::is_whitespace) || !site.contains('.') {
            return Err("PARTY_BAD_WEBSITE: enter a web address such as example.com".into());
        }
    }
    if input.terms_days.is_some_and(|d| !(0..=365).contains(&d)) {
        return Err("PARTY_BAD_TERMS: terms are 0 to 365 days".into());
    }
    let profile = check_profile(pool, t, id, &input).await?;
    let terms_days = profile.terms_days.or(input.terms_days);
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
    if let Some(level) = input.price_level_id.filter(|_| t == "customers") {
        let active: Option<bool> = sqlx::query_scalar("SELECT is_active FROM price_levels WHERE id = ?1").bind(level).fetch_optional(pool).await.map_err(db_err)?;
        let unchanged = sqlx::query_scalar::<_, Option<i64>>("SELECT price_level_id FROM customers WHERE id = ?1").bind(id).fetch_optional(pool).await.map_err(db_err)?.flatten()
            == input.price_level_id;
        match active {
            None => return Err("PARTY_BAD_PRICE_LEVEL: that price level no longer exists".into()),
            Some(false) if !unchanged => return Err("PARTY_INACTIVE_PRICE_LEVEL: that price level is inactive".into()),
            _ => {}
        }
    }
    let delivery = crate::sales_prefs::parse_delivery(input.preferred_delivery.as_deref())?;
    let job_status = crate::sales_prefs::parse_job_status(input.job_status.as_deref())?;
    let keep_tax_id = input.keep_tax_id || t == "customers";
    if !keep_tax_id && !access.can_see() {
        return Err("TAX_ID_LOCKED: only users who can see sensitive data can change a tax ID".into());
    }
    let (tax_id, tax_id_masked) = if keep_tax_id { (None, None) } else { tax_ids::store(access, id, input.tax_id.as_deref())? };
    let query = if t == "customers" {
        sqlx::query(
            "UPDATE customers SET name = ?1, company_name = ?2, email = ?3, phone = ?4, billing_address = ?5, terms_days = ?6, notes = ?7,
                    tax_rate_id = ?8, is_tax_exempt = ?9, tax_exemption = ?10, form_style_id = ?11, sales_rep_id = ?13, price_level_id = ?14,
                    terms_id = ?15, customer_type_id = ?16, job_type_id = ?17, preferred_delivery = ?18, job_status = ?19,
                    shipping_address = ?20, contact_name = ?21, alt_contact_name = ?22, alt_phone = ?23, fax = ?24, website = ?25,
                    account_number = ?26, credit_limit_cents = ?27, resale_number = ?28 WHERE id = ?12",
        )
        .bind(name)
        .bind(clean(&input.company_name))
        .bind(email)
        .bind(clean(&input.phone))
        .bind(clean(&input.address))
        .bind(terms_days)
        .bind(clean(&input.notes))
        .bind(input.tax_rate_id)
        .bind(input.is_tax_exempt)
        .bind(clean(&input.tax_exemption))
        .bind(input.form_style_id)
        .bind(id)
        .bind(input.sales_rep_id)
        .bind(input.price_level_id)
        .bind(input.terms_id)
        .bind(input.customer_type_id)
        .bind(input.job_type_id)
        .bind(delivery)
        .bind(job_status)
        .bind(clean(&input.shipping_address).map(|a| crate::sales_header::normalize_address(&a)))
        .bind(clean(&input.contact_name))
        .bind(clean(&input.alt_contact_name))
        .bind(clean(&input.alt_phone))
        .bind(clean(&input.fax))
        .bind(clean(&input.website))
        .bind(clean(&input.account_number))
        .bind(input.credit_limit_cents)
        .bind(clean(&input.resale_number))
    } else {
        sqlx::query(
            "UPDATE vendors SET name = ?1, company_name = ?2, email = ?3, phone = ?4, address = ?5, terms_days = ?6, notes = ?7,
                    tax_id = CASE WHEN ?11 THEN tax_id ELSE ?8 END, tax_id_masked = CASE WHEN ?11 THEN tax_id_masked ELSE ?12 END,
                    is_1099 = ?9, terms_id = ?13, vendor_type_id = ?14 WHERE id = ?10",
        )
        .bind(name)
        .bind(clean(&input.company_name))
        .bind(email)
        .bind(clean(&input.phone))
        .bind(clean(&input.address))
        .bind(terms_days)
        .bind(clean(&input.notes))
        .bind(tax_id)
        .bind(input.is_1099)
        .bind(id)
        .bind(keep_tax_id)
        .bind(tax_id_masked)
        .bind(input.terms_id)
        .bind(input.vendor_type_id)
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

/// What the profile-list choices on a record decide.
struct Profile {
    /// The chosen term's days.
    terms_days: Option<i64>,
}

/// Checks the term and types chosen for a customer or vendor: each must exist, be on the right list
/// and be active unless the record already has it.
async fn check_profile(pool: &SqlitePool, t: &str, id: i64, input: &PartyDetailsInput) -> Result<Profile, String> {
    use crate::profile_lists::{check_entry, check_terms, ProfileList};
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let current: (Option<i64>, Option<i64>, Option<i64>) = if t == "customers" {
        sqlx::query_as("SELECT terms_id, customer_type_id, job_type_id FROM customers WHERE id = ?1")
    } else {
        sqlx::query_as("SELECT terms_id, vendor_type_id, NULL FROM vendors WHERE id = ?1")
    }
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(db_err)?
    .unwrap_or_default();
    let terms_days = match input.terms_id {
        Some(terms) => Some(check_terms(&mut conn, terms, current.0, "PARTY").await?),
        None => None,
    };
    if t == "customers" {
        if input.vendor_type_id.is_some() {
            return Err("PARTY_BAD_TYPE: a customer has a customer type, not a vendor type".into());
        }
        if let Some(kind) = input.customer_type_id {
            check_entry(&mut conn, ProfileList::CustomerType, kind, current.1, "PARTY").await?;
        }
        if let Some(kind) = input.job_type_id {
            check_entry(&mut conn, ProfileList::JobType, kind, current.2, "PARTY").await?;
        }
    } else {
        if input.customer_type_id.is_some() || input.job_type_id.is_some() {
            return Err("PARTY_BAD_TYPE: a vendor has a vendor type".into());
        }
        if let Some(kind) = input.vendor_type_id {
            check_entry(&mut conn, ProfileList::VendorType, kind, current.1, "PARTY").await?;
        }
    }
    Ok(Profile { terms_days })
}

pub async fn set_party_active(pool: &SqlitePool, kind: &str, id: i64, active: bool, access: Access<'_>) -> Result<PartyDetails, String> {
    let t = table(kind)?;
    sqlx::query(&format!("UPDATE {t} SET is_active = ?1 WHERE id = ?2")).bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_party(pool, kind, id, access).await
}

/// What a copy of a customer takes from the original: everything but the name, the account and
/// resale numbers (each customer's own) and the history.
const CUSTOMER_COPIED: &str = "email, company_name, phone, billing_address, terms_days, notes, tax_rate_id, is_tax_exempt, \
     tax_exemption, parent_id, form_style_id, sales_rep_id, price_level_id, terms_id, customer_type_id, job_type_id, \
     shipping_address, contact_name, alt_contact_name, alt_phone, fax, website, credit_limit_cents, preferred_delivery, job_status";
/// A copy of a vendor leaves out the tax ID and the account number we have with them.
const VENDOR_COPIED: &str = "email, company_name, phone, address, terms_days, is_1099, notes, terms_id, vendor_type_id";

/// Duplicate on the Customer and Vendor lists: a new, active record named `name`, filled from
/// `id` (contact details, terms, types, tax settings, custom fields), with no transactions. A
/// copy of a job stays a job of the same customer, so its name must start with that customer's.
pub async fn duplicate_party(pool: &SqlitePool, kind: &str, id: i64, name: &str, access: Access<'_>) -> Result<PartyDetails, String> {
    let t = table(kind)?;
    // "Acme : Kitchen" is the job "Acme:Kitchen", as when a job is typed on a form.
    let name = if kind == "customer" { name.split(':').map(str::trim).collect::<Vec<_>>().join(":") } else { name.trim().to_string() };
    let name = name.as_str();
    if name.is_empty() || (kind == "customer" && name.split(':').any(str::is_empty)) {
        return Err("PARTY_NO_NAME: enter a name for the copy".into());
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let parent: Option<Option<String>> = if kind == "customer" {
        sqlx::query_scalar("SELECT p.name FROM customers c LEFT JOIN customers p ON p.id = c.parent_id WHERE c.id = ?1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?
    } else {
        sqlx::query_scalar("SELECT NULL FROM vendors WHERE id = ?1").bind(id).fetch_optional(&mut *tx).await.map_err(db_err)?
    };
    let Some(parent) = parent else {
        return Err(crate::ledger::rollback(tx, format!("PARTY_NOT_FOUND: that {kind} does not exist")).await);
    };
    if kind == "customer" && parent.is_none() && name.contains(':') {
        return Err(crate::ledger::rollback(tx, "PARTY_DUP_JOB: a copy of a customer is a customer; use Add job… to make a job".into()).await);
    }
    if let Some(parent) = parent {
        // `get` keeps a multi-byte name from being cut mid-character.
        let under = name.len() > parent.len() + 1
            && name.get(..parent.len()).is_some_and(|head| head.eq_ignore_ascii_case(&parent))
            && name.as_bytes()[parent.len()] == b':';
        if !under {
            return Err(crate::ledger::rollback(tx, format!("PARTY_DUP_JOB: a copy of a job stays under {parent}: name it {parent}:…")).await);
        }
    }
    let copied = if kind == "customer" { CUSTOMER_COPIED } else { VENDOR_COPIED };
    let inserted = sqlx::query_scalar::<_, i64>(&format!("INSERT INTO {t} (name, {copied}) SELECT ?1, {copied} FROM {t} WHERE id = ?2 RETURNING id"))
        .bind(name)
        .bind(id)
        .fetch_one(&mut *tx)
        .await;
    let new_id = match inserted {
        Ok(new_id) => new_id,
        Err(err) if err.as_database_error().is_some_and(|e| e.is_unique_violation()) => {
            return Err(crate::ledger::rollback(tx, format!("PARTY_DUPLICATE: there is already a {kind} named '{name}'")).await);
        }
        Err(err) => return Err(crate::ledger::rollback(tx, db_err(err)).await),
    };
    sqlx::query(
        "INSERT INTO custom_field_values (field_id, record_id, value)
         SELECT v.field_id, ?1, v.value FROM custom_field_values v JOIN custom_fields f ON f.id = v.field_id
         WHERE v.record_id = ?2 AND f.list = ?3",
    )
    .bind(new_id)
    .bind(id)
    .bind(kind.to_uppercase())
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    get_party(pool, kind, new_id, access).await
}

/// Changes a product's name, description, default rate and income account. Documents already
/// posted keep what they were posted with.
#[cfg(test)]
pub async fn update_product(pool: &SqlitePool, id: i64, input: ProductInput) -> Result<ProductView, String> {
    update_item(pool, id, input, None).await
}

/// Like [`update_product`], also changing the item's kind when `kind` is given.
pub async fn update_item(pool: &SqlitePool, id: i64, input: ProductInput, kind: Option<crate::sales::ItemKind>) -> Result<ProductView, String> {
    if let Some(kind) = &kind {
        kind.validate()?;
    }
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
    if let Some(kind) = kind {
        sqlx::query("UPDATE products_services SET item_type = ?1, percent_ppm = ?2 WHERE id = ?3")
            .bind(&kind.item_type)
            .bind(kind.percent_ppm)
            .bind(id)
            .execute(pool)
            .await
            .map_err(db_err)?;
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
