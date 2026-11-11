//! The refunds and write-offs taken from one customer credit (gaps row T73), for the credit memo's
//! own screen: each with its account, reference (the check number of a refund paid by check) and
//! whether it was voided, so it can be printed or voided from there.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::db_err;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CreditRefundView {
    pub id: i64,
    pub refund_date: String,
    pub amount_cents: i64,
    pub account_id: i64,
    pub account_name: String,
    /// BANK, CREDIT_CARD, INCOME…: a refund from a bank account can print a check.
    pub account_type: String,
    pub reference: Option<String>,
    pub is_write_off: bool,
    pub journal_entry_id: i64,
    pub is_voided: bool,
}

/// A customer credit's refunds and write-offs, oldest first.
pub async fn list_customer_credit_refunds(pool: &SqlitePool, credit_id: i64) -> Result<Vec<CreditRefundView>, String> {
    sqlx::query_as(
        "SELECT r.id, r.refund_date, r.amount_cents, r.account_id, a.full_name AS account_name, a.account_type, r.reference,
                r.is_write_off, r.journal_entry_id, e.is_voided
         FROM credit_refunds r JOIN v_chart_of_accounts a ON a.id = r.account_id JOIN journal_entries e ON e.id = r.journal_entry_id
         WHERE r.credit_memo_id = ?1 ORDER BY r.refund_date, r.id",
    )
    .bind(credit_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credits::{create_credit_memo, refund_customer_credit, write_off_customer_credit, CreditMemoInput, RefundCreditInput};
    use crate::db::test_pool;
    use crate::money::Cents;
    use chrono::NaiveDate;

    #[tokio::test]
    async fn a_credit_lists_its_refunds_and_write_offs_and_a_voided_one_shows_as_voided() {
        let pool = test_pool().await;
        let account = |n: &'static str| {
            let pool = pool.clone();
            async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
        };
        let (checking, services) = (account("1010").await, account("4100").await);
        let d = |day| NaiveDate::from_ymd_opt(2026, 7, day).unwrap();
        let memo = create_credit_memo(
            &pool,
            CreditMemoInput {
                customer_name: "Acme".into(),
                credit_number: None,
                credit_date: d(1),
                memo: None,
                refund_account_id: None,
                tax_rate_id: None,
                lines: vec![crate::sales::SalesLineInput { product_service_id: None, description: "Return".into(), quantity_e4: 10_000, rate_cents: crate::money::Cents::from(10_000), income_account_id: services, class_id: None, is_taxable: false, service_date: None }],
                header: Default::default(),
                form_lines: Vec::new(),
            },
        )
        .await
        .unwrap();
        refund_customer_credit(&pool, RefundCreditInput { credit_id: memo.id, account_id: checking, refund_date: d(5), amount_cents: Cents(6_000), reference: Some("1042".into()) }).await.unwrap();
        write_off_customer_credit(&pool, RefundCreditInput { credit_id: memo.id, account_id: services, refund_date: d(6), amount_cents: Cents(4_000), reference: None }).await.unwrap();

        let list = list_customer_credit_refunds(&pool, memo.id).await.unwrap();
        let rows: Vec<(i64, &str, Option<&str>, bool, bool)> = list.iter().map(|r| (r.amount_cents, r.account_type.as_str(), r.reference.as_deref(), r.is_write_off, r.is_voided)).collect();
        assert_eq!(rows, vec![(6_000, "BANK", Some("1042"), false, false), (4_000, "INCOME", None, true, false)]);

        // Voiding the refund from the list gives the credit back.
        crate::ledger::void_transaction(&pool, list[0].journal_entry_id).await.unwrap();
        assert!(list_customer_credit_refunds(&pool, memo.id).await.unwrap()[0].is_voided);
        let left: i64 = sqlx::query_scalar("SELECT remaining_cents FROM v_credit_memo_balances WHERE credit_memo_id = ?1").bind(memo.id).fetch_one(&pool).await.unwrap();
        assert_eq!(left, 6_000);
    }
}
