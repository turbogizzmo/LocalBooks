//! Smart category and payee suggestions for bank feed lines, learned from this company file.
//!
//! The method follows beancount's smart_importer: learn from transactions already in the books and
//! predict the other side of each new bank line. Here that is a nearest-neighbour model written
//! out by hand, with no ML runtime. Each past transaction becomes a sparse vector:
//!
//! - TF-IDF-weighted words and word pairs from its normalized description,
//! - its exact amount, so a recurring $15.99 subscription finds its earlier charges,
//! - its amount band (about 50% wide), bank account and day of the month, at fixed small weights.
//!
//! A new line is compared, by cosine similarity, with the past lines that share at least one word or
//! its exact amount. Its closest neighbours vote for a category, and the winning neighbours vote for a
//! payee. Money in and money out never match each other.
//!
//! Explicit rules always come first; this model only fills lines no rule matches, and only suggests:
//! nothing is approved without the user. The model lives in memory, is rebuilt from the company file
//! when that changes, and grows by one example on each approval. Nothing leaves the computer.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::db_err;

/// Neighbours that vote on a suggestion.
const K: usize = 10;
/// Below this similarity a past line is not considered similar at all; when a rule already chose
/// the category and only a payee is wanted, a weaker likeness is enough.
const MIN_SIMILARITY: f32 = 0.2;
const MIN_SIMILARITY_FOR_PAYEE: f32 = 0.05;
/// Below this confidence the model makes no suggestion and the line stays uncategorized.
const MIN_CONFIDENCE: f32 = 0.25;
/// A best neighbour this similar (or more) counts as a full match when computing confidence.
const FULL_MATCH: f32 = 0.45;
/// The exact amount counts half as much as a description word of the same rarity, so a bill whose
/// amount changes every month is not pushed away from its earlier ones.
const W_AMOUNT: f32 = 0.5;
/// Word pairs add precision but repeat their words, so they count a little less.
const W_PAIR: f32 = 0.7;
/// A word this company file has never seen (a new store's city, a reference code) makes a line less
/// like everything, but should not drown out the words it shares with past lines.
const W_UNSEEN: f32 = 0.5;
/// Weights of the features every line has, relative to a word seen in about a third of all lines.
const W_BAND: f32 = 1.0;
const W_BANK: f32 = 0.4;
const W_DAY: f32 = 0.4;
/// Words this common (share of all lines) are not used to find candidates, only to score them.
const STOP_SHARE: f32 = 0.1;
/// Candidates come from at most this many of the most recent lines sharing each word, which keeps a
/// batch fast however long the history grows.
const MAX_POSTINGS: usize = 1_000;

/// One past transaction to learn from: a bank line approved in Bank Feeds, or an expense or deposit
/// entered directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub bank_account_id: i64,
    pub date: NaiveDate,
    pub description: String,
    /// Negative is money out, as on bank lines.
    pub amount_cents: i64,
    pub account_id: i64,
    pub payee: Option<String>,
}

/// The past transaction a suggestion is most like, shown as "because similar to …".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarTo {
    pub description: String,
    pub date: String,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prediction {
    pub account_id: i64,
    /// 0-100.
    pub confidence: u8,
    pub similar_to: SimilarTo,
    pub payee: Option<String>,
}

/// `HIGH`, `MEDIUM` or `LOW`, for the badge next to a suggestion.
pub fn confidence_level(confidence: u8) -> &'static str {
    match confidence {
        80.. => "HIGH",
        50.. => "MEDIUM",
        _ => "LOW",
    }
}

/// Words that say how a payment was made rather than who it was to.
const NOISE: &[&str] = &[
    "POS", "DEBIT", "CREDIT", "PURCHASE", "CARD", "CHECKCARD", "ACH", "ONLINE", "RECURRING", "WEB", "PMT", "PPD", "CCD",
    "SQ", "TST", "VISA", "MC", "INC", "LLC", "CO", "THE", "WWW", "COM", "XX", "XXX", "XXXX", "DES", "INDN", "ORIG",
];

/// Description words: uppercase runs of letters (with `&` and `'`), keeping none that contain digits
/// (store numbers, reference codes) and none of the payment-method noise words.
pub fn tokens(description: &str) -> Vec<String> {
    description
        .to_uppercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '&' || c == '\''))
        .map(|w| w.trim_matches(|c| c == '&' || c == '\''))
        .filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2 && !w.chars().any(|c| c.is_ascii_digit()) && !NOISE.contains(w))
        .map(str::to_string)
        .collect()
}

/// The keyed features of a line: words, word pairs and the exact amount.
fn keyed_features(description: &str, amount_cents: i64) -> Vec<String> {
    let words = tokens(description);
    let mut out: Vec<String> = words.iter().map(|w| format!("W:{w}")).collect();
    out.extend(words.windows(2).map(|p| format!("B:{} {}", p[0], p[1])));
    out.push(format!("A:{amount_cents}"));
    out.sort();
    out.dedup();
    out
}

fn kind_weight(feature: &str) -> f32 {
    match feature.as_bytes().first() {
        Some(b'A') => W_AMOUNT,
        Some(b'B') => W_PAIR,
        _ => 1.0,
    }
}

/// About 50% wide: 10.00 and 14.00 share a band, 10.00 and 16.00 are neighbours.
fn amount_band(amount_cents: i64) -> i16 {
    ((amount_cents.unsigned_abs().max(1) as f64).ln() / 1.5f64.ln()).floor() as i16
}

struct Row {
    features: Vec<u32>,
    band: i16,
    bank_account_id: i64,
    day: u8,
    outflow: bool,
    account_id: i64,
    /// Its category may be suggested (the account is active).
    eligible: bool,
    payee: Option<u32>,
    description: String,
    date: NaiveDate,
    amount_cents: i64,
}

/// A line to suggest for.
#[derive(Debug, Clone)]
pub struct Query<'a> {
    pub bank_account_id: i64,
    pub date: NaiveDate,
    pub description: &'a str,
    pub amount_cents: i64,
}

#[derive(Default)]
pub struct Model {
    vocab: HashMap<String, u32>,
    df: Vec<u32>,
    postings: Vec<Vec<u32>>,
    rows: Vec<Row>,
    payees: Vec<String>,
    payee_ids: HashMap<String, u32>,
    /// Categories the model may suggest (active accounts); `None` allows any.
    allowed: Option<HashSet<i64>>,
}

impl Model {
    pub fn train(examples: impl IntoIterator<Item = Example>, allowed: Option<HashSet<i64>>) -> Self {
        let mut model = Model { allowed, ..Model::default() };
        for e in examples {
            model.add(e);
        }
        model
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Learns one more example (an approval), without retraining on the rest.
    pub fn add(&mut self, e: Example) {
        let idx = self.rows.len() as u32;
        let mut features = Vec::new();
        for f in keyed_features(&e.description, e.amount_cents) {
            let id = match self.vocab.get(&f) {
                Some(&id) => id,
                None => {
                    let id = self.df.len() as u32;
                    self.vocab.insert(f, id);
                    self.df.push(0);
                    self.postings.push(Vec::new());
                    id
                }
            };
            self.df[id as usize] += 1;
            self.postings[id as usize].push(idx);
            features.push(id);
        }
        // Ids come in first-seen order, not the keys' order; scoring binary-searches them.
        features.sort_unstable();
        let payee = e.payee.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(|p| match self.payee_ids.get(p) {
            Some(&id) => id,
            None => {
                let id = self.payees.len() as u32;
                self.payees.push(p.to_string());
                self.payee_ids.insert(p.to_string(), id);
                id
            }
        });
        self.rows.push(Row {
            features,
            band: amount_band(e.amount_cents),
            bank_account_id: e.bank_account_id,
            day: e.date.day() as u8,
            outflow: e.amount_cents < 0,
            eligible: self.allowed.as_ref().is_none_or(|a| a.contains(&e.account_id)),
            account_id: e.account_id,
            payee,
            description: e.description,
            date: e.date,
            amount_cents: e.amount_cents,
        });
    }

    fn idf(&self, df: u32) -> f32 {
        ((self.rows.len() as f32 + 1.0) / (df as f32 + 1.0)).ln() + 1.0
    }

    /// Each feature's weight: its IDF, scaled by its kind.
    fn vocab_weights(&self) -> Vec<f32> {
        let mut w: Vec<f32> = self.df.iter().map(|&d| self.idf(d)).collect();
        for (f, &id) in &self.vocab {
            w[id as usize] *= kind_weight(f);
        }
        w
    }

    /// Suggests for many lines at once; `only_account[i]` restricts line `i` to that category (a rule
    /// already chose it, and only the payee is wanted).
    pub fn predict_batch(&self, queries: &[Query], only_account: &[Option<i64>]) -> Vec<Option<Prediction>> {
        if self.rows.is_empty() {
            return vec![None; queries.len()];
        }
        let idf: Vec<f32> = self.vocab_weights();
        let fixed = W_BAND * W_BAND + W_BANK * W_BANK + W_DAY * W_DAY;
        let norms: Vec<f32> = self.rows.iter().map(|r| (r.features.iter().map(|&f| idf[f as usize].powi(2)).sum::<f32>() + fixed).sqrt()).collect();
        let mut dots = vec![0f32; self.rows.len()];
        let mut touched: Vec<u32> = Vec::new();
        queries
            .iter()
            .enumerate()
            .map(|(i, q)| self.predict_one(q, only_account.get(i).copied().flatten(), &idf, &norms, &mut dots, &mut touched))
            .collect()
    }

    pub fn predict(&self, q: &Query) -> Option<Prediction> {
        self.predict_batch(std::slice::from_ref(q), &[None]).pop().flatten()
    }

    fn predict_one(&self, q: &Query, only: Option<i64>, idf: &[f32], norms: &[f32], dots: &mut [f32], touched: &mut Vec<u32>) -> Option<Prediction> {
        let n = self.rows.len() as f32;
        let unseen = self.idf(0);
        let mut known: Vec<(u32, f32)> = Vec::new();
        let mut q_norm2 = W_BAND * W_BAND + W_BANK * W_BANK + W_DAY * W_DAY;
        for f in keyed_features(q.description, q.amount_cents) {
            match self.vocab.get(&f) {
                Some(&id) => {
                    let w = idf[id as usize];
                    q_norm2 += w * w;
                    known.push((id, w));
                }
                None => {
                    let w = unseen * kind_weight(&f) * W_UNSEEN;
                    q_norm2 += w * w;
                }
            }
        }
        // Very common words only find candidates when nothing rarer does.
        let rare: Vec<(u32, f32)> = known.iter().copied().filter(|&(id, _)| (self.df[id as usize] as f32) < STOP_SHARE * n).collect();
        let (seek, score_only) = if rare.is_empty() { (known.clone(), Vec::new()) } else { (rare, known.clone()) };

        touched.clear();
        for &(id, w) in &seek {
            let postings = &self.postings[id as usize];
            for &r in &postings[postings.len().saturating_sub(MAX_POSTINGS)..] {
                if dots[r as usize] == 0.0 {
                    touched.push(r);
                }
                dots[r as usize] += w * w;
            }
        }
        let seek_ids: HashSet<u32> = seek.iter().map(|&(id, _)| id).collect();
        let common: Vec<(u32, f32)> = score_only.into_iter().filter(|(id, _)| !seek_ids.contains(id)).collect();

        let q_norm = q_norm2.sqrt();
        let (outflow, band, day) = (q.amount_cents < 0, amount_band(q.amount_cents), q.date.day() as i32);
        let mut scored: Vec<(f32, u32)> = Vec::with_capacity(touched.len());
        for &r in touched.iter() {
            let mut dot = std::mem::take(&mut dots[r as usize]);
            let row = &self.rows[r as usize];
            if row.outflow != outflow || !row.eligible || row.account_id == q.bank_account_id || only.is_some_and(|a| a != row.account_id) {
                continue;
            }
            for &(id, w) in &common {
                if row.features.binary_search(&id).is_ok() {
                    dot += w * w;
                }
            }
            dot += match (row.band - band).abs() {
                0 => W_BAND * W_BAND,
                1 => W_BAND * W_BAND * 0.5,
                _ => 0.0,
            };
            if row.bank_account_id == q.bank_account_id {
                dot += W_BANK * W_BANK;
            }
            let gap = (row.day as i32 - day).abs();
            dot += match gap.min(31 - gap) {
                0 => W_DAY * W_DAY,
                1 | 2 => W_DAY * W_DAY * 0.5,
                _ => 0.0,
            };
            let sim = dot / (q_norm * norms[r as usize]);
            if sim >= if only.is_some() { MIN_SIMILARITY_FOR_PAYEE } else { MIN_SIMILARITY } {
                scored.push((sim, r));
            }
        }
        if scored.is_empty() {
            return None;
        }
        // Most similar first; among equals, the most recent.
        let by_rank = |a: &(f32, u32), b: &(f32, u32)| b.0.total_cmp(&a.0).then(self.rows[b.1 as usize].date.cmp(&self.rows[a.1 as usize].date)).then(b.1.cmp(&a.1));
        if scored.len() > K {
            scored.select_nth_unstable_by(K - 1, by_rank);
            scored.truncate(K);
        }
        scored.sort_by(by_rank);

        // Neighbours vote with the square of their similarity, so close matches dominate.
        let mut votes: Vec<(i64, f32)> = Vec::new();
        for &(sim, r) in &scored {
            let account = self.rows[r as usize].account_id;
            match votes.iter_mut().find(|(a, _)| *a == account) {
                Some(v) => v.1 += sim * sim,
                None => votes.push((account, sim * sim)),
            }
        }
        let total: f32 = votes.iter().map(|v| v.1).sum();
        let &(account_id, weight) = votes.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
        let (best_sim, best) = scored.iter().copied().find(|&(_, r)| self.rows[r as usize].account_id == account_id)?;
        let confidence = (weight / total) * (best_sim / FULL_MATCH).min(1.0);
        if only.is_none() && confidence < MIN_CONFIDENCE {
            return None;
        }

        let mut payee_votes: Vec<(u32, f32)> = Vec::new();
        let mut voters = 0f32;
        for &(sim, r) in scored.iter().filter(|&&(_, r)| self.rows[r as usize].account_id == account_id) {
            voters += sim * sim;
            if let Some(p) = self.rows[r as usize].payee {
                match payee_votes.iter_mut().find(|(id, _)| *id == p) {
                    Some(v) => v.1 += sim * sim,
                    None => payee_votes.push((p, sim * sim)),
                }
            }
        }
        let payee = payee_votes
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, w)| *w >= 0.5 * voters)
            .map(|(p, _)| self.payees[*p as usize].clone());

        let row = &self.rows[best as usize];
        Some(Prediction {
            account_id,
            confidence: (confidence * 100.0).round().clamp(1.0, 100.0) as u8,
            similar_to: SimilarTo { description: row.description.clone(), date: row.date.to_string(), amount_cents: row.amount_cents },
            payee,
        })
    }
}

// ------------------------------------------------------------------------------------------------
// Training data from the company file, and the in-memory cache
// ------------------------------------------------------------------------------------------------

/// Approved bank lines, then expenses and deposit lines entered directly (not through Bank Feeds),
/// oldest first. An expense counts as its largest split line.
pub async fn load_examples(pool: &SqlitePool) -> Result<Vec<Example>, String> {
    let rows: Vec<(i64, NaiveDate, String, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT b.bank_account_id, b.txn_date, b.description, b.amount_cents, b.category_account_id, v.name
         FROM bank_transactions b
         LEFT JOIN expenses x ON x.journal_entry_id = b.journal_entry_id
         LEFT JOIN vendors v ON v.id = x.vendor_id
         WHERE b.status = 'APPROVED'
         UNION ALL
         SELECT x.payment_account_id, x.expense_date, TRIM(COALESCE(v.name, '') || ' ' || COALESCE(x.memo, '')), -x.total_cents,
                (SELECT l.account_id FROM expense_lines l WHERE l.expense_id = x.id ORDER BY l.amount_cents DESC, l.line_number LIMIT 1),
                v.name
         FROM expenses x LEFT JOIN vendors v ON v.id = x.vendor_id
         WHERE x.is_voided = 0 AND x.journal_entry_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM bank_transactions b WHERE b.journal_entry_id = x.journal_entry_id)
         UNION ALL
         SELECT d.bank_account_id, d.deposit_date, TRIM(COALESCE(l.memo, '') || ' ' || COALESCE(d.memo, '')), l.amount_cents, l.account_id, NULL
         FROM deposit_lines l JOIN deposits d ON d.id = l.deposit_id
         WHERE d.is_voided = 0 AND l.is_voided = 0 AND d.journal_entry_id IS NOT NULL
           AND l.invoice_payment_id IS NULL AND l.sales_receipt_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM bank_transactions b WHERE b.journal_entry_id = d.journal_entry_id)
         ORDER BY 2",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(bank_account_id, date, description, amount_cents, account_id, payee)| Example { bank_account_id, date, description, amount_cents, account_id, payee })
        .collect())
}

/// Changes whenever the training data could have: lines approved or returned to review, expenses
/// and deposits entered or voided, accounts made active or inactive.
async fn signature(pool: &SqlitePool) -> Result<String, String> {
    sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) || ':' || IFNULL(MAX(id), 0) FROM bank_transactions WHERE status = 'APPROVED')
             || '|' || (SELECT COUNT(*) || ':' || IFNULL(MAX(id), 0) || ':' || IFNULL(SUM(is_voided), 0) FROM expenses)
             || '|' || (SELECT COUNT(*) || ':' || IFNULL(MAX(id), 0) || ':' || IFNULL(SUM(is_voided), 0) FROM deposits)
             || '|' || (SELECT COUNT(*) || ':' || IFNULL(SUM(id), 0) FROM chart_of_accounts WHERE is_active = 1)",
    )
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

async fn active_accounts(pool: &SqlitePool) -> Result<HashSet<i64>, String> {
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE is_active = 1").fetch_all(pool).await.map_err(db_err)?;
    Ok(ids.into_iter().collect())
}

struct Cached {
    /// Which company file, and what its training data looked like when the model was built.
    file: String,
    signature: String,
    model: Model,
}

/// The model for the open company file, kept in memory between calls.
#[derive(Default)]
pub struct Suggester {
    cached: tokio::sync::Mutex<Option<Cached>>,
}

impl Suggester {
    /// Runs `f` with the current model, rebuilding it first if the file's history changed.
    pub async fn with_model<T>(&self, pool: &SqlitePool, file: &str, f: impl FnOnce(&Model) -> T) -> Result<T, String> {
        let sig = signature(pool).await?;
        let mut cached = self.cached.lock().await;
        if !cached.as_ref().is_some_and(|c| c.file == file && c.signature == sig) {
            let model = Model::train(load_examples(pool).await?, Some(active_accounts(pool).await?));
            *cached = Some(Cached { file: file.to_string(), signature: sig, model });
        }
        Ok(f(&cached.as_ref().expect("model built above").model))
    }

    /// Learns one approval. `before` is the file's signature taken just before approving: when the
    /// cached model matches it, the example is added and the model stays current; otherwise the
    /// next suggestion rebuilds it.
    pub async fn learn(&self, pool: &SqlitePool, file: &str, before: &str, example: Example) -> Result<(), String> {
        let mut cached = self.cached.lock().await;
        if let Some(c) = cached.as_mut().filter(|c| c.file == file && c.signature == before) {
            c.model.add(example);
            c.signature = signature(pool).await?;
        }
        Ok(())
    }

    pub async fn signature(pool: &SqlitePool) -> Result<String, String> {
        signature(pool).await
    }

    /// The number of examples in the cached model, for tests.
    #[cfg(test)]
    pub async fn cached_len(&self) -> Option<usize> {
        self.cached.lock().await.as_ref().map(|c| c.model.len())
    }
}

#[cfg(test)]
mod tests;
