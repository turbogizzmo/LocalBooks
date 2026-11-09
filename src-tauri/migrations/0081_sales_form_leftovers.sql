-- 0081_sales_form_leftovers.sql
-- Sales form leftovers (gap T23):
-- * a Service Date on each invoice and sales receipt line, printed in its own column (`IS`, not
--   `=`, so an impossible date such as 2026-13-99, where date() is NULL, fails the check);
-- * the form style a document prints with, picked on the form itself (NULL: the customer's style,
--   or the default);
-- * a printed credit memo, so form_templates also holds 'CREDIT_MEMO' layouts.

ALTER TABLE invoice_items ADD COLUMN service_date TEXT
    CHECK (service_date IS NULL OR (service_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' AND date(service_date) IS service_date));
ALTER TABLE sales_receipt_lines ADD COLUMN service_date TEXT
    CHECK (service_date IS NULL OR (service_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' AND date(service_date) IS service_date));

ALTER TABLE invoices ADD COLUMN form_style_id INTEGER REFERENCES form_styles (id) ON DELETE SET NULL;
ALTER TABLE sales_receipts ADD COLUMN form_style_id INTEGER REFERENCES form_styles (id) ON DELETE SET NULL;
ALTER TABLE estimates ADD COLUMN form_style_id INTEGER REFERENCES form_styles (id) ON DELETE SET NULL;

CREATE TABLE form_templates_new (
    style_id       INTEGER NOT NULL REFERENCES form_styles (id) ON DELETE CASCADE,
    form_type      TEXT    NOT NULL CHECK (form_type IN ('INVOICE', 'ESTIMATE', 'SALES_RECEIPT', 'STATEMENT', 'CREDIT_MEMO')),
    template_json  TEXT    NOT NULL CHECK (json_valid(template_json)),
    updated_at     TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (style_id, form_type)
) STRICT;
INSERT INTO form_templates_new (style_id, form_type, template_json, updated_at)
SELECT style_id, form_type, template_json, updated_at FROM form_templates;
DROP TABLE form_templates;
ALTER TABLE form_templates_new RENAME TO form_templates;

-- Credit memos can be emailed and queued for Email later like the other forms. The sent history
-- stays append-only: the table is rebuilt with its rows and its triggers.
CREATE TABLE form_email_log_new (
    id           INTEGER PRIMARY KEY,
    form_type    TEXT    NOT NULL CHECK (form_type IN ('INVOICE', 'ESTIMATE', 'SALES_RECEIPT', 'STATEMENT', 'CREDIT_MEMO')),
    document_id  INTEGER NOT NULL,
    sent_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    recipients   TEXT    NOT NULL CHECK (length(recipients) > 0),
    method       TEXT    NOT NULL CHECK (method IN ('MAIL_APP', 'SMTP')),
    subject      TEXT    NOT NULL
) STRICT;
INSERT INTO form_email_log_new (id, form_type, document_id, sent_at, recipients, method, subject)
SELECT id, form_type, document_id, sent_at, recipients, method, subject FROM form_email_log ORDER BY id;
DROP TABLE form_email_log;
ALTER TABLE form_email_log_new RENAME TO form_email_log;
CREATE INDEX ix_form_email_log_document ON form_email_log (form_type, document_id);
CREATE TRIGGER form_email_log_no_update BEFORE UPDATE ON form_email_log
BEGIN
    SELECT RAISE(ABORT, 'EMAIL_LOG_APPEND_ONLY: sent history cannot be changed');
END;
CREATE TRIGGER form_email_log_no_delete BEFORE DELETE ON form_email_log
BEGIN
    SELECT RAISE(ABORT, 'EMAIL_LOG_APPEND_ONLY: sent history cannot be deleted');
END;

CREATE TABLE form_email_queue_new (
    form_type    TEXT    NOT NULL CHECK (form_type IN ('INVOICE', 'ESTIMATE', 'SALES_RECEIPT', 'STATEMENT', 'CREDIT_MEMO')),
    document_id  INTEGER NOT NULL,
    queued_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (form_type, document_id)
) STRICT;
INSERT INTO form_email_queue_new (form_type, document_id, queued_at) SELECT form_type, document_id, queued_at FROM form_email_queue;
DROP TABLE form_email_queue;
ALTER TABLE form_email_queue_new RENAME TO form_email_queue;
