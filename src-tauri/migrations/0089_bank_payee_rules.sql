-- 0089_bank_payee_rules.sql
-- Renaming rules for downloaded payees (gap T53).
--
-- A bank describes the same payee many ways ("AMZN MKTP US*2K4", "AMAZON.COM*7Q1"). A renaming rule
-- says: when a downloaded description begins with, contains or ends with this text (case aside),
-- use this name as its payee. Rules are settings, not postings: they can be edited and removed, and
-- only change the name suggested for lines still waiting for review.

CREATE TABLE bank_payee_rules (
    id          INTEGER PRIMARY KEY,
    match_kind  TEXT    NOT NULL CHECK (match_kind IN ('BEGINS', 'CONTAINS', 'ENDS')),
    pattern     TEXT    NOT NULL CHECK (length(trim(pattern)) BETWEEN 1 AND 100),
    payee_name  TEXT    NOT NULL CHECK (length(trim(payee_name)) BETWEEN 1 AND 100),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_bank_payee_rules ON bank_payee_rules (match_kind, pattern COLLATE NOCASE);
