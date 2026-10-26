-- Clears "tax IDs" that are really QuickBooks' own encrypted values (see `tax_ids::is_quickbooks_encrypted`).
--
-- Newer QuickBooks company files store vendor tax IDs encrypted with QuickBooks' key, as base64 text.
-- Before LocalBooks learned to skip them (migration 0042's release), importing such a file copied that
-- text into vendors.tax_id, where it can never be read and would show as a masked "tax ID" and on
-- the 1099 report. The rule is deliberately narrow, matching only that encoding: at least 24
-- characters, nothing but base64 characters, a letter and a digit, and a `/` or `+` or a trailing
-- `=`. Real tax IDs and notes such as "n/a" or "W-9 requested" are left alone.
--
-- Values already encrypted by LocalBooks (`lb1:…`) can't be checked here; `tax_ids::protect_all`
-- clears those when users are turned on or someone who can see sensitive data signs in.

UPDATE vendors SET tax_id = NULL, tax_id_masked = NULL
WHERE tax_id NOT LIKE 'lb1:%'
  AND length(trim(tax_id)) >= 24
  AND trim(tax_id) NOT GLOB '*[^A-Za-z0-9+/=]*'
  AND trim(tax_id) GLOB '*[A-Za-z]*'
  AND trim(tax_id) GLOB '*[0-9]*'
  AND (instr(tax_id, '/') > 0 OR instr(tax_id, '+') > 0 OR trim(tax_id) GLOB '*=');

UPDATE ledger_import_parties SET tax_id = NULL
WHERE tax_id IS NOT NULL
  AND length(trim(tax_id)) >= 24
  AND trim(tax_id) NOT GLOB '*[^A-Za-z0-9+/=]*'
  AND trim(tax_id) GLOB '*[A-Za-z]*'
  AND trim(tax_id) GLOB '*[0-9]*'
  AND (instr(tax_id, '/') > 0 OR instr(tax_id, '+') > 0 OR trim(tax_id) GLOB '*=');
