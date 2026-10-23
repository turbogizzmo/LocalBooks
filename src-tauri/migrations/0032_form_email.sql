-- Emailed forms: the sent history and the "Email later" queue.
--
-- form_email_log is append-only (the triggers refuse changes): one row each time a form was sent
-- over SMTP or opened in the user's mail app, with the recipients. It never touches the posted
-- transaction. form_email_queue holds the forms marked "Email later" until they are emailed or
-- unmarked. Statements are keyed by customer id; other forms by their own id.

CREATE TABLE form_email_log (
    id           INTEGER PRIMARY KEY,
    form_type    TEXT    NOT NULL CHECK (form_type IN ('INVOICE', 'ESTIMATE', 'SALES_RECEIPT', 'STATEMENT')),
    document_id  INTEGER NOT NULL,
    sent_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    recipients   TEXT    NOT NULL CHECK (length(recipients) > 0),
    method       TEXT    NOT NULL CHECK (method IN ('MAIL_APP', 'SMTP')),
    subject      TEXT    NOT NULL
) STRICT;
CREATE INDEX ix_form_email_log_document ON form_email_log (form_type, document_id);

CREATE TRIGGER form_email_log_no_update BEFORE UPDATE ON form_email_log
BEGIN
    SELECT RAISE(ABORT, 'EMAIL_LOG_APPEND_ONLY: sent history cannot be changed');
END;
CREATE TRIGGER form_email_log_no_delete BEFORE DELETE ON form_email_log
BEGIN
    SELECT RAISE(ABORT, 'EMAIL_LOG_APPEND_ONLY: sent history cannot be deleted');
END;

CREATE TABLE form_email_queue (
    form_type    TEXT    NOT NULL CHECK (form_type IN ('INVOICE', 'ESTIMATE', 'SALES_RECEIPT', 'STATEMENT')),
    document_id  INTEGER NOT NULL,
    queued_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (form_type, document_id)
) STRICT;
