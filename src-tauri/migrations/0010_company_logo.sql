-- 0010_company_logo.sql
-- Company logo for printed invoices and statements, stored as a data URL
-- ("data:image/png;base64,...") so the company file stays self-contained and portable.
-- Empty means no logo. Written only through the logo commands, which validate the image.

INSERT INTO system_preferences (key, value) VALUES ('company_logo_base64', '');
