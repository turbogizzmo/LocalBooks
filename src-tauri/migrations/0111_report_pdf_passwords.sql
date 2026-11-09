-- 0111_report_pdf_passwords.sql
-- Password-protected report PDFs (gap T60): a schedule's PDFs can open only with a password. The
-- password itself is kept in this computer's credential store, never in the company file; this
-- column holds only the random name it is stored under (NULL: the PDFs have no password).

ALTER TABLE report_schedules ADD COLUMN pdf_password_key TEXT CHECK (pdf_password_key IS NULL OR length(pdf_password_key) BETWEEN 1 AND 64);
