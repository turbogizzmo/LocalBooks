"""Prints the document-trigger part of 0099_merge_names.sql (gap T30): every BEFORE UPDATE trigger on
the tables a merge repoints, recreated so it lets a merge being applied move one name column and
nothing else. Run from the repo root as `python3 scripts/migrations/merge_exceptions.py .`; it reads
the schema the migrations before 0099 build. Regenerate 0099 when an unshipped migration before it
adds or recreates one of those triggers."""
import sqlite3, glob, re, sys
root = sys.argv[1]
db = sqlite3.connect(':memory:')
for f in sorted(glob.glob(f'{root}/src-tauri/migrations/*.sql')):
    if '0099' in f: continue
    db.executescript(open(f).read())

# table -> {column: merge kind}; a name column ('NAME:' kind) moves with its id's merge.
REPOINT = {
    'invoices': {'customer_id': 'CUSTOMER', 'customer_name': 'NAME:CUSTOMER'},
    'sales_receipts': {'customer_id': 'CUSTOMER'},
    'estimates': {'customer_id': 'CUSTOMER'},
    'credit_memos': {'customer_id': 'CUSTOMER'},
    'bounced_checks': {'customer_id': 'CUSTOMER'},
    'bounced_sales_receipts': {'customer_id': 'CUSTOMER'},
    'deposit_lines': {'customer_id': 'CUSTOMER', 'vendor_id': 'VENDOR'},
    'bill_lines': {'customer_id': 'CUSTOMER', 'product_service_id': 'ITEM'},
    'expense_lines': {'customer_id': 'CUSTOMER'},
    'vendor_credit_lines': {'customer_id': 'CUSTOMER'},
    'bills': {'vendor_id': 'VENDOR'},
    'expenses': {'vendor_id': 'VENDOR'},
    'vendor_credits': {'vendor_id': 'VENDOR'},
    'purchase_orders': {'vendor_id': 'VENDOR'},
    'ach_entries': {'vendor_id': 'VENDOR'},
    'invoice_items': {'product_service_id': 'ITEM'},
    'sales_receipt_lines': {'product_service_id': 'ITEM'},
    'estimate_lines': {'product_service_id': 'ITEM', 'income_account_id': 'ACCOUNT'},
    'credit_memo_lines': {'product_service_id': 'ITEM'},
    'purchase_order_lines': {'product_service_id': 'ITEM', 'account_id': 'ACCOUNT'},
    'invoice_form_lines': {'product_service_id': 'ITEM'},
    'sales_receipt_form_lines': {'product_service_id': 'ITEM'},
    'estimate_form_lines': {'product_service_id': 'ITEM', 'account_id': 'ACCOUNT'},
    # credit_memo_form_lines (0114, after 0099) carries its own merge exception.
}
tables = {r[0] for r in db.execute("select name from sqlite_master where type='table'")}
out = []
for table, cols in REPOINT.items():
    if table not in tables:
        continue  # a table a sibling migration adds; wrapped when it lands
    columns = [r[1] for r in db.execute(f"pragma table_info('{table}')")]
    for c in cols:
        assert c in columns, (table, c)
    parts = []
    for c in columns:
        kind = cols.get(c)
        if kind is None:
            parts.append(f"NEW.{c} IS OLD.{c}")
        elif kind.startswith('NAME:'):
            parts.append(f"(NEW.{c} IS OLD.{c} OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = '{kind[5:]}' AND m.from_name = OLD.{c} AND m.to_name = NEW.{c}))")
        else:
            parts.append(f"(NEW.{c} IS OLD.{c} OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = '{kind}' AND m.from_id = OLD.{c} AND m.to_id = NEW.{c}))")
    repoint = "\n       AND ".join(parts)
    for name, sql in db.execute("select name, sql from sqlite_master where type='trigger' and tbl_name = ?", (table,)):
        if not re.search(r'BEFORE\s+UPDATE', sql, re.I):
            continue
        m = re.match(r'(?is)(CREATE TRIGGER\s+\S+\s+BEFORE\s+UPDATE(?:\s+OF\s+[^\n]*?)?\s+ON\s+\S+)\s*(?:WHEN\s+(.*?))?\s*(BEGIN\b.*)$', sql)
        assert m, name
        head, when, body = m.group(1), m.group(2), m.group(3)
        guard = f"NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)\n       AND {repoint})"
        new_when = f"{guard}\n  AND ({when.strip()})" if when else guard
        out.append(f"DROP TRIGGER {name};\n{head}\nWHEN {new_when}\n{body};\n")
print("\n".join(out))
