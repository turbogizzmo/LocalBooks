-- Form designer: one layout per printed form type (invoice, estimate), as JSON the app validates
-- (title, paper, font, accent color, logo placement, visible details and columns, messages).
-- No row means the built-in default layout.

CREATE TABLE form_templates (
    form_type      TEXT NOT NULL PRIMARY KEY CHECK (form_type IN ('INVOICE', 'ESTIMATE')),
    template_json  TEXT NOT NULL CHECK (json_valid(template_json)),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
