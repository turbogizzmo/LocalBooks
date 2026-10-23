-- Form styles: several named sets of form templates, one of them the default, with a style per
-- customer. Forms also cover sales receipts and customer statements now.
--
-- * form_styles: named styles; exactly one is the default (the one 0030's templates become).
-- * form_templates: now one layout per style and form type. No row means the built-in default
--   layout for that form type. Rebuilt with the documented procedure (SQLite cannot change a
--   primary key or CHECK in place); nothing references it.
-- * customers.form_style_id: the style a customer's forms use (NULL = the default style).

CREATE TABLE form_styles (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL UNIQUE COLLATE NOCASE CHECK (length(trim(name)) BETWEEN 1 AND 60),
    is_default  INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_form_styles_one_default ON form_styles (is_default) WHERE is_default = 1;

INSERT INTO form_styles (id, name, is_default) VALUES (1, 'Standard', 1);

CREATE TABLE form_templates_new (
    style_id       INTEGER NOT NULL REFERENCES form_styles (id) ON DELETE CASCADE,
    form_type      TEXT    NOT NULL CHECK (form_type IN ('INVOICE', 'ESTIMATE', 'SALES_RECEIPT', 'STATEMENT')),
    template_json  TEXT    NOT NULL CHECK (json_valid(template_json)),
    updated_at     TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (style_id, form_type)
) STRICT;

INSERT INTO form_templates_new (style_id, form_type, template_json, updated_at)
SELECT 1, form_type, template_json, updated_at FROM form_templates;

DROP TABLE form_templates;
ALTER TABLE form_templates_new RENAME TO form_templates;

ALTER TABLE customers ADD COLUMN form_style_id INTEGER REFERENCES form_styles (id) ON DELETE SET NULL;
