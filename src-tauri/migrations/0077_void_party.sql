-- 0077_void_party.sql
-- A void's reversing entry belongs to the same customer or vendor as the entry it reverses (gap T38).
--
-- A void posts a reversing journal entry (source JOURNAL, `reverses_entry_id` set). 0066's view
-- looked only at the entry's own source, so a reversal had no name: a voided customer payment
-- stayed on the customer's statement with no offsetting line, and registers and reports by name
-- left the reversal unnamed. The view now names each entry from the entry it reverses, if any.

DROP VIEW v_entry_party;
CREATE VIEW v_entry_party AS
SELECT e.id AS journal_entry_id,
       CASE x.source_type
           WHEN 'INVOICE'         THEN (SELECT c.name FROM invoices d JOIN customers c ON c.id = d.customer_id WHERE d.id = x.source_id)
           WHEN 'RECEIVE_PAYMENT' THEN (SELECT c.name FROM invoice_payments p JOIN invoices d ON d.id = p.invoice_id JOIN customers c ON c.id = d.customer_id WHERE p.id = x.source_id)
           WHEN 'SALES_RECEIPT'   THEN (SELECT c.name FROM sales_receipts d JOIN customers c ON c.id = d.customer_id WHERE d.id = x.source_id)
           WHEN 'CREDIT_MEMO'     THEN (SELECT c.name FROM credit_memos d JOIN customers c ON c.id = d.customer_id WHERE d.id = x.source_id)
           WHEN 'BILL'            THEN (SELECT v.name FROM bills d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = x.source_id)
           WHEN 'BILL_PAYMENT'    THEN (SELECT v.name FROM bill_payments p JOIN bills d ON d.id = p.bill_id JOIN vendors v ON v.id = d.vendor_id WHERE p.id = x.source_id)
           WHEN 'VENDOR_CREDIT'   THEN (SELECT v.name FROM vendor_credits d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = x.source_id)
           WHEN 'EXPENSE'         THEN (SELECT v.name FROM expenses d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = x.source_id)
           WHEN 'CHECK'           THEN (SELECT v.name FROM expenses d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = x.source_id)
           WHEN 'JOURNAL'         THEN COALESCE(
               (SELECT c.name FROM credit_refunds r JOIN credit_memos d ON d.id = r.credit_memo_id JOIN customers c ON c.id = d.customer_id WHERE r.journal_entry_id = x.id),
               (SELECT v.name FROM vendor_credit_refunds r JOIN vendor_credits d ON d.id = r.vendor_credit_id JOIN vendors v ON v.id = d.vendor_id WHERE r.journal_entry_id = x.id),
               (SELECT c.name FROM bounced_checks b JOIN customers c ON c.id = b.customer_id WHERE b.journal_entry_id = x.id))
       END AS party_name
FROM journal_entries e
LEFT JOIN journal_entries o ON o.id = e.reverses_entry_id
JOIN journal_entries x ON x.id = COALESCE(o.id, e.id);
