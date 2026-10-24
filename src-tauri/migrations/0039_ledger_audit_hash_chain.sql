-- Tamper-evident ledger audit chain: every row records actor and hash links.
ALTER TABLE ledger_audit_log ADD COLUMN actor TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE ledger_audit_log ADD COLUMN prev_hash TEXT;
ALTER TABLE ledger_audit_log ADD COLUMN row_hash TEXT;

CREATE INDEX IF NOT EXISTS ix_ledger_audit_row_hash ON ledger_audit_log (row_hash);
