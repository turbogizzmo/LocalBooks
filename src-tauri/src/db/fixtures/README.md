# Database fixtures

Synthetic data written for Tillhouse's tests. No customer or vendor data from anywhere else.

## pre-0177.localbooks.gz

A company file at schema version 175, written by the build before gap R24: sqlx 0.8.6 with
libsqlite3-sys 0.30.1, which is SQLCipher 4.5.7 on SQLite 3.45.3. It is unencrypted, in
`journal_mode = DELETE` and vacuumed, then gzipped. That SQLite still kept a subquery's JSON as
JSON, so its ledger snapshots hold `lines` and `changed` as arrays. That is the case migration 0177
has to keep verifying on SQLite 3.54 (`db::tests::a_file_from_before_the_json_fix_still_verifies`).

What's in it:
- the seeded chart of accounts and one class, "North shop";
- three posted entries: a cash sale whose memo and line memo use non-ASCII text and quotes, a sale
  with a classed line, and an entry that was voided, plus its reversal;
- a customer and a vendor whose details changed, giving LIST_CHANGE rows with a `changed` list.

Its hash chain, `verify_ledger_audit` and the array form of `lines` were all checked before it was
written. To make it again, check out a commit from before R24 (for example `620d574b`). Then
write the same records with `Db::open_at`, `ledger::post_journal_entry`,
`ledger::void_transaction` and plain `INSERT`/`UPDATE` on `customers` and `vendors`, and finish
with `PRAGMA wal_checkpoint(TRUNCATE)`, `PRAGMA journal_mode = DELETE` and `VACUUM`.
