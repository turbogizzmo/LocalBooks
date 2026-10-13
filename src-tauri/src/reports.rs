//! Read-side queries: chart of accounts with balances, trial balance, and the dashboard series.

use std::collections::HashMap;

use chrono::{Datelike, Months, NaiveDate};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::db_err;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: i64,
    pub account_number: Option<String>,
    pub full_name: String,
    pub account_type: String,
    pub account_type_name: String,
    pub classification: String,
    pub is_active: bool,
    pub is_system: bool,
    pub balance_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialBalanceRow {
    pub account_id: i64,
    pub account_number: Option<String>,
    pub full_name: String,
    pub account_type_name: String,
    pub debit_cents: i64,
    pub credit_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialBalance {
    pub rows: Vec<TrialBalanceRow>,
    pub total_debit_cents: i64,
    pub total_credit_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthPoint {
    /// `YYYY-MM`
    pub month: String,
    pub assets_cents: i64,
    pub liabilities_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub trial_balance: TrialBalance,
    pub series: Vec<MonthPoint>,
    pub total_assets_cents: i64,
    pub total_liabilities_cents: i64,
}

pub async fn list_accounts(pool: &SqlitePool) -> Result<Vec<AccountView>, String> {
    sqlx::query_as::<_, AccountView>(
        "SELECT c.id, c.account_number, c.full_name, c.account_type, c.account_type_name, c.classification,
                c.is_active, c.is_system, b.balance_cents
         FROM v_chart_of_accounts c
         JOIN v_account_balances b ON b.account_id = c.id
         ORDER BY c.type_sort_order, COALESCE(c.account_number, ''), c.full_name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn trial_balance(pool: &SqlitePool) -> Result<TrialBalance, String> {
    let rows: Vec<(i64, Option<String>, String, String, i64, i64)> = sqlx::query_as(
        "SELECT c.id, c.account_number, c.full_name, c.account_type_name, b.total_debit_cents, b.total_credit_cents
         FROM v_chart_of_accounts c
         JOIN v_account_balances b ON b.account_id = c.id
         WHERE b.total_debit_cents <> b.total_credit_cents
         ORDER BY c.type_sort_order, COALESCE(c.account_number, ''), c.full_name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let mut tb = TrialBalance { rows: Vec::with_capacity(rows.len()), total_debit_cents: 0, total_credit_cents: 0 };
    for (account_id, account_number, full_name, account_type_name, debits, credits) in rows {
        let net = debits - credits;
        let (debit_cents, credit_cents) = if net > 0 { (net, 0) } else { (0, -net) };
        tb.total_debit_cents += debit_cents;
        tb.total_credit_cents += credit_cents;
        tb.rows.push(TrialBalanceRow { account_id, account_number, full_name, account_type_name, debit_cents, credit_cents });
    }
    Ok(tb)
}

/// Month-end asset and liability balances for the 12 months ending with `today`'s month.
pub async fn asset_liability_series(pool: &SqlitePool, today: NaiveDate) -> Result<Vec<MonthPoint>, String> {
    let current = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("first of month is valid");
    let start = current - Months::new(11);
    let end = current + Months::new(1);

    // Net debit movement per (month, classification); everything before the window collapses into OPEN.
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT CASE WHEN p.entry_date < ?1 THEN 'OPEN' ELSE substr(p.entry_date, 1, 7) END AS bucket,
                t.classification,
                SUM(p.debit_cents) - SUM(p.credit_cents) AS net
         FROM v_posted_lines p
         JOIN chart_of_accounts a ON a.id = p.account_id
         JOIN account_types t ON t.code = a.account_type
         WHERE t.classification IN ('ASSET', 'LIABILITY') AND p.entry_date < ?2
         GROUP BY bucket, t.classification",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let movement: HashMap<(String, String), i64> =
        rows.into_iter().map(|(bucket, class, net)| ((bucket, class), net)).collect();
    let net = |bucket: &str, class: &str| movement.get(&(bucket.to_string(), class.to_string())).copied().unwrap_or(0);

    let mut assets = net("OPEN", "ASSET");
    let mut liabilities = -net("OPEN", "LIABILITY");
    let mut series = Vec::with_capacity(12);
    for i in 0..12 {
        let month = (start + Months::new(i)).format("%Y-%m").to_string();
        assets += net(&month, "ASSET");
        liabilities -= net(&month, "LIABILITY");
        series.push(MonthPoint { month, assets_cents: assets, liabilities_cents: liabilities });
    }
    Ok(series)
}

pub async fn dashboard(pool: &SqlitePool, today: NaiveDate) -> Result<Dashboard, String> {
    let (total_assets_cents, total_liabilities_cents): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(CASE WHEN classification = 'ASSET' THEN balance_cents END), 0),
                COALESCE(SUM(CASE WHEN classification = 'LIABILITY' THEN balance_cents END), 0)
         FROM v_account_balances",
    )
    .fetch_one(pool)
    .await
    .map_err(db_err)?;

    Ok(Dashboard {
        trial_balance: trial_balance(pool).await?,
        series: asset_liability_series(pool, today).await?,
        total_assets_cents,
        total_liabilities_cents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

    async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
            .bind(number)
            .fetch_one(pool)
            .await
            .expect("seeded account exists")
    }

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    #[tokio::test]
    async fn trial_balance_balances_and_series_accumulates() {
        let pool = test_pool().await;
        let (checking, card, owner, rent) = (
            account_id(&pool, "1010").await,
            account_id(&pool, "2100").await,
            account_id(&pool, "3100").await,
            account_id(&pool, "6300").await,
        );
        let post = |date, lines| post_journal_entry(&pool, JournalEntry { id: None, date, description: String::new(), lines });

        post(ymd(2025, 6, 1), vec![JournalLine::debit(checking, 500_000), JournalLine::credit(owner, 500_000)])
            .await
            .unwrap();
        post(ymd(2026, 9, 5), vec![JournalLine::debit(rent, 120_000), JournalLine::credit(card, 120_000)])
            .await
            .unwrap();
        post(ymd(2026, 10, 2), vec![JournalLine::debit(card, 20_000), JournalLine::credit(checking, 20_000)])
            .await
            .unwrap();

        let tb = trial_balance(&pool).await.unwrap();
        assert_eq!(tb.total_debit_cents, tb.total_credit_cents);
        assert_eq!(tb.total_debit_cents, 600_000);
        assert_eq!(tb.rows.len(), 4);

        let series = asset_liability_series(&pool, ymd(2026, 10, 13)).await.unwrap();
        assert_eq!(series.len(), 12);
        assert_eq!(series[0], MonthPoint { month: "2025-11".into(), assets_cents: 500_000, liabilities_cents: 0 });
        assert_eq!(series[10], MonthPoint { month: "2026-09".into(), assets_cents: 500_000, liabilities_cents: 120_000 });
        assert_eq!(series[11], MonthPoint { month: "2026-10".into(), assets_cents: 480_000, liabilities_cents: 100_000 });

        let dash = dashboard(&pool, ymd(2026, 10, 13)).await.unwrap();
        assert_eq!((dash.total_assets_cents, dash.total_liabilities_cents), (480_000, 100_000));
    }
}
