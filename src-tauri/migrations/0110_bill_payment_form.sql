-- 0110_bill_payment_form.sql
-- A bill payment form of its own (gap T46): the stub (remittance advice) for a payment to a vendor,
-- laid out with the form designer like the customer forms, previewed, printed, saved as PDF and
-- emailed to the vendor. Its document is a bill payment (with the other bills paid on the same
-- check), and its layouts, sent history and Email later queue sit with the other forms', so the
-- three tables' form types gain 'BILL_PAYMENT'. As in 0081, each is rebuilt with its rows (and the
-- sent history with its append-only triggers); nothing references them.

CREATE TABLE form_templates_new (
    style_id       INTEGER NOT NULL REFERENCES form_styles (id) ON DELETE CASCADE,
    form_type      TEXT    NOT NULL CHECK (form_type IN ('INVOICE', 'ESTIMATE', 'SALES_RECEIPT', 'STATEMENT', 'CREDIT_MEMO', 'BILL_PAYMENT')),
    template_json  TEXT    NOT NULL CHECK (json_valid(template_json)),
    updated_at     TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (style_id, form_type)
) STRICT;
INSERT INTO form_templates_new (style_id, form_type, template_json, updated_at)
SELECT style_id, form_type, template_json, updated_at FROM form_templates;
DROP TABLE form_templates;
ALTER TABLE form_templates_new RENAME TO form_templates;

-- The sent history stays append-only: the table is rebuilt with its rows and its triggers.
CREATE TABLE form_email_log_new (
    id           INTEGER PRIMARY KEY,
    form_type    TEXT    NOT NULL CHECK (form_type IN ('INVOICE', 'ESTIMATE', 'SALES_RECEIPT', 'STATEMENT', 'CREDIT_MEMO', 'BILL_PAYMENT')),
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
    form_type    TEXT    NOT NULL CHECK (form_type IN ('INVOICE', 'ESTIMATE', 'SALES_RECEIPT', 'STATEMENT', 'CREDIT_MEMO', 'BILL_PAYMENT')),
    document_id  INTEGER NOT NULL,
    queued_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (form_type, document_id)
) STRICT;
INSERT INTO form_email_queue_new (form_type, document_id, queued_at) SELECT form_type, document_id, queued_at FROM form_email_queue;
DROP TABLE form_email_queue;
ALTER TABLE form_email_queue_new RENAME TO form_email_queue;
