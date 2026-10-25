use super::*;

use sqlx::{Connection, SqliteConnection};

// One small book, saved both ways: built in SQLite here, written by hand in XML below. Both
// must read to the same ledger.
//
// Accounts: Assets (placeholder) with Checking, a euro bank account and a stock account;
// receivables and payables; Liabilities:Visa; Income:Consulting; Expenses:Office (hidden);
// Equity:Opening Balances; and a template tree for a scheduled transaction.
// Business: customer Acme Corp with a job Website; vendor Paper Co; terms Net 30; an invoice to
// the job (2 hours at $50) paid $60 through a payment lot and a link transaction; a bill (3 at
// $10) paid in full straight into its lot.
// Problems: an amount of $1.005, a euro transaction with no dollar amount on its euro split, a
// stock purchase, and a transaction that does not balance.

const SCHEMA: &str = "
CREATE TABLE books (guid text(32) PRIMARY KEY NOT NULL, root_account_guid text(32) NOT NULL, root_template_guid text(32) NOT NULL);
CREATE TABLE commodities (guid text(32) PRIMARY KEY NOT NULL, namespace text(2048) NOT NULL, mnemonic text(2048) NOT NULL,
    fullname text(2048), cusip text(2048), fraction integer NOT NULL, quote_flag integer NOT NULL, quote_source text(2048), quote_tz text(2048));
CREATE TABLE accounts (guid text(32) PRIMARY KEY NOT NULL, name text(2048) NOT NULL, account_type text(2048) NOT NULL,
    commodity_guid text(32), commodity_scu integer NOT NULL, non_std_scu integer NOT NULL, parent_guid text(32),
    code text(2048), description text(2048), hidden integer, placeholder integer);
CREATE TABLE transactions (guid text(32) PRIMARY KEY NOT NULL, currency_guid text(32) NOT NULL, num text(2048) NOT NULL,
    post_date text(19), enter_date text(19), description text(2048));
CREATE TABLE splits (guid text(32) PRIMARY KEY NOT NULL, tx_guid text(32) NOT NULL, account_guid text(32) NOT NULL,
    memo text(2048) NOT NULL, action text(2048) NOT NULL, reconcile_state text(1) NOT NULL, reconcile_date text(19),
    value_num bigint NOT NULL, value_denom bigint NOT NULL, quantity_num bigint NOT NULL, quantity_denom bigint NOT NULL, lot_guid text(32));
CREATE TABLE slots (id integer PRIMARY KEY AUTOINCREMENT NOT NULL, obj_guid text(32) NOT NULL, name text(4096) NOT NULL,
    slot_type integer NOT NULL, int64_val bigint, string_val text(4096), double_val float8, timespec_val text(19),
    guid_val text(32), numeric_val_num bigint, numeric_val_denom bigint, gdate_val text(8));
CREATE TABLE lots (guid text(32) PRIMARY KEY NOT NULL, account_guid text(32), is_closed integer NOT NULL);
CREATE TABLE customers (guid text(32) PRIMARY KEY NOT NULL, name text(2048) NOT NULL, id text(2048) NOT NULL, notes text(2048) NOT NULL,
    active integer NOT NULL, discount_num bigint NOT NULL, discount_denom bigint NOT NULL, credit_num bigint NOT NULL,
    credit_denom bigint NOT NULL, currency text(32) NOT NULL, tax_override integer NOT NULL, addr_name text(1024),
    addr_addr1 text(1024), addr_addr2 text(1024), addr_addr3 text(1024), addr_addr4 text(1024), addr_phone text(128),
    addr_fax text(128), addr_email text(256), shipaddr_name text(1024), shipaddr_addr1 text(1024), shipaddr_addr2 text(1024),
    shipaddr_addr3 text(1024), shipaddr_addr4 text(1024), shipaddr_phone text(128), shipaddr_fax text(128),
    shipaddr_email text(256), terms text(32), tax_included integer, taxtable text(32));
CREATE TABLE vendors (guid text(32) PRIMARY KEY NOT NULL, name text(2048) NOT NULL, id text(2048) NOT NULL, notes text(2048) NOT NULL,
    currency text(32) NOT NULL, active integer NOT NULL, tax_override integer NOT NULL, addr_name text(1024),
    addr_addr1 text(1024), addr_addr2 text(1024), addr_addr3 text(1024), addr_addr4 text(1024), addr_phone text(128),
    addr_fax text(128), addr_email text(256), terms text(32), tax_inc text(2048), tax_table text(32));
CREATE TABLE jobs (guid text(32) PRIMARY KEY NOT NULL, id text(2048) NOT NULL, name text(2048) NOT NULL, reference text(2048) NOT NULL,
    active integer NOT NULL, owner_type integer, owner_guid text(32));
CREATE TABLE invoices (guid text(32) PRIMARY KEY NOT NULL, id text(2048) NOT NULL, date_opened text(19), date_posted text(19),
    notes text(2048) NOT NULL, active integer NOT NULL, currency text(32) NOT NULL, owner_type integer, owner_guid text(32),
    terms text(32), billing_id text(2048), post_txn text(32), post_lot text(32), post_acc text(32), billto_type integer,
    billto_guid text(32), charge_amt_num bigint, charge_amt_denom bigint);
CREATE TABLE entries (guid text(32) PRIMARY KEY NOT NULL, date text(19) NOT NULL, date_entered text(19), description text(2048),
    action text(2048), notes text(2048), quantity_num bigint, quantity_denom bigint, i_acct text(32), i_price_num bigint,
    i_price_denom bigint, i_discount_num bigint, i_discount_denom bigint, invoice text(32), i_disc_type text(2048),
    i_disc_how text(2048), i_taxable integer, i_taxincluded integer, i_taxtable text(32), b_acct text(32), b_price_num bigint,
    b_price_denom bigint, bill text(32), b_taxable integer, b_taxincluded integer, b_taxtable text(32), b_paytype integer,
    billable integer, billto_type integer, billto_guid text(32), order_guid text(32));
CREATE TABLE billterms (guid text(32) PRIMARY KEY NOT NULL, name text(2048) NOT NULL, description text(2048) NOT NULL,
    refcount integer NOT NULL, invisible integer NOT NULL, parent text(32), type text(2048) NOT NULL, duedays integer,
    discountdays integer, discount_num bigint, discount_denom bigint, cutoff integer);
";

const DATA: &str = "
INSERT INTO books VALUES ('book', 'root', 'troot');
INSERT INTO commodities VALUES
    ('usd', 'CURRENCY', 'USD', 'US Dollar', '840', 100, 1, 'currency', ''),
    ('eur', 'CURRENCY', 'EUR', 'Euro', '978', 100, 1, 'currency', ''),
    ('aapl', 'NASDAQ', 'AAPL', 'Apple Inc.', '', 10000, 0, NULL, NULL);
INSERT INTO accounts (guid, name, account_type, commodity_guid, commodity_scu, non_std_scu, parent_guid, code, description, hidden, placeholder) VALUES
    ('root', 'Root Account', 'ROOT', 'usd', 100, 0, NULL, '', '', 0, 0),
    ('assets', 'Assets', 'ASSET', 'usd', 100, 0, 'root', '', '', 0, 1),
    ('checking', 'Checking', 'BANK', 'usd', 100, 0, 'assets', '1010', '', 0, 0),
    ('euro', 'Euro Account', 'BANK', 'eur', 100, 0, 'assets', '', '', 0, 0),
    ('brokerage', 'Brokerage', 'STOCK', 'aapl', 10000, 0, 'assets', '', '', 0, 0),
    ('ar', 'Accounts Receivable', 'RECEIVABLE', 'usd', 100, 0, 'root', '', '', 0, 0),
    ('ap', 'Accounts Payable', 'PAYABLE', 'usd', 100, 0, 'root', '', '', 0, 0),
    ('liab', 'Liabilities', 'LIABILITY', 'usd', 100, 0, 'root', '', '', 0, 1),
    ('visa', 'Visa', 'CREDIT', 'usd', 100, 0, 'liab', '', '', 0, 0),
    ('income', 'Income', 'INCOME', 'usd', 100, 0, 'root', '', '', 0, 1),
    ('consulting', 'Consulting', 'INCOME', 'usd', 100, 0, 'income', '', '', 0, 0),
    ('expenses', 'Expenses', 'EXPENSE', 'usd', 100, 0, 'root', '', '', 0, 1),
    ('office', 'Office', 'EXPENSE', 'usd', 100, 0, 'expenses', '', '', 1, 0),
    ('equity', 'Equity', 'EQUITY', 'usd', 100, 0, 'root', '', '', 0, 1),
    ('opening', 'Opening Balances', 'EQUITY', 'usd', 100, 0, 'equity', '', '', 0, 0),
    ('troot', 'Template Root', 'ROOT', NULL, 0, 0, NULL, '', '', 0, 0),
    ('tmpl', 'sx-account', 'BANK', 'usd', 100, 0, 'troot', '', '', 0, 0);
INSERT INTO transactions VALUES
    ('t1', 'usd', '', '2024-01-01 10:59:00', '2024-01-01 10:59:00', 'Opening balance'),
    ('t2', 'usd', '000001', '2024-01-05 10:59:00', '2024-01-05 10:59:00', 'Acme Corp'),
    ('t3', 'usd', '', '2024-01-20 10:59:00', '2024-01-20 10:59:00', 'Acme payment'),
    ('t4', 'usd', '', '2024-01-20 10:59:00', '2024-01-20 10:59:00', 'Link'),
    ('t5', 'usd', 'B-1', '20240110105900', '20240110105900', 'Paper Co'),
    ('t6', 'usd', '', '2024-01-25 10:59:00', '2024-01-25 10:59:00', 'Pay Paper Co'),
    ('t7', 'usd', '101', '2024-01-31 23:00:00', '2024-02-01 09:00:00', 'Staples'),
    ('t8', 'usd', '', '2024-02-02 10:59:00', '2024-02-02 10:59:00', 'Coffee shop'),
    ('t9', 'usd', '', '2024-02-03 10:59:00', '2024-02-03 10:59:00', 'Pay the card'),
    ('t10', 'usd', '', '2024-02-04 10:59:00', '2024-02-04 10:59:00', 'Fuel'),
    ('t11', 'eur', '', '2024-02-05 10:59:00', '2024-02-05 10:59:00', 'Paris client'),
    ('t12', 'usd', '', '2024-02-06 10:59:00', '2024-02-06 10:59:00', 'Fund the euro account'),
    ('t13', 'usd', '', '2024-02-07 10:59:00', '2024-02-07 10:59:00', 'Buy AAPL'),
    ('t14', 'usd', '', '2024-02-08 10:59:00', '2024-02-08 10:59:00', 'Typo'),
    ('tt', 'usd', '', '2024-01-01 10:59:00', '2024-01-01 10:59:00', 'Monthly rent');
INSERT INTO splits VALUES
    ('s1a', 't1', 'checking', '', '', 'n', NULL, 100000, 100, 100000, 100, NULL),
    ('s1b', 't1', 'opening', '', '', 'n', NULL, -100000, 100, -100000, 100, NULL),
    ('s2a', 't2', 'ar', '', '', 'n', NULL, 10000, 100, 10000, 100, 'l1'),
    ('s2b', 't2', 'consulting', 'Consulting hours', '', 'n', NULL, -10000, 100, -10000, 100, NULL),
    ('s3a', 't3', 'checking', '', '', 'n', NULL, 6000, 100, 6000, 100, NULL),
    ('s3b', 't3', 'ar', '', '', 'n', NULL, -6000, 100, -6000, 100, 'l2'),
    ('s4a', 't4', 'ar', '', '', 'n', NULL, 6000, 100, 6000, 100, 'l2'),
    ('s4b', 't4', 'ar', '', '', 'n', NULL, -6000, 100, -6000, 100, 'l1'),
    ('s5a', 't5', 'office', 'Printer paper', '', 'n', NULL, 3000, 100, 3000, 100, NULL),
    ('s5b', 't5', 'ap', '', '', 'n', NULL, -3000, 100, -3000, 100, 'l3'),
    ('s6a', 't6', 'ap', '', '', 'n', NULL, 3000, 100, 3000, 100, 'l3'),
    ('s6b', 't6', 'checking', '', '', 'n', NULL, -3000, 100, -3000, 100, NULL),
    ('s7a', 't7', 'office', 'Paper', '', 'n', NULL, 1234, 100, 1234, 100, NULL),
    ('s7b', 't7', 'checking', '', '', 'n', NULL, -1234, 100, -1234, 100, NULL),
    ('s8a', 't8', 'office', '', '', 'n', NULL, 2000, 100, 2000, 100, NULL),
    ('s8b', 't8', 'visa', '', '', 'n', NULL, -2000, 100, -2000, 100, NULL),
    ('s9a', 't9', 'visa', '', '', 'n', NULL, 2000, 100, 2000, 100, NULL),
    ('s9b', 't9', 'checking', '', '', 'n', NULL, -2000, 100, -2000, 100, NULL),
    ('s10a', 't10', 'office', '', '', 'n', NULL, 1005, 1000, 1005, 1000, NULL),
    ('s10b', 't10', 'checking', '', '', 'n', NULL, -1005, 1000, -1005, 1000, NULL),
    ('s11a', 't11', 'euro', '', '', 'n', NULL, 5000, 100, 5000, 100, NULL),
    ('s11b', 't11', 'consulting', '', '', 'n', NULL, -5000, 100, -5500, 100, NULL),
    ('s12a', 't12', 'euro', '', '', 'n', NULL, 11000, 100, 10000, 100, NULL),
    ('s12b', 't12', 'checking', '', '', 'n', NULL, -11000, 100, -11000, 100, NULL),
    ('s13a', 't13', 'brokerage', '', 'Buy', 'n', NULL, 50000, 100, 20000, 10000, NULL),
    ('s13b', 't13', 'checking', '', '', 'n', NULL, -50000, 100, -50000, 100, NULL),
    ('s14a', 't14', 'office', '', '', 'n', NULL, 1000, 100, 1000, 100, NULL),
    ('s14b', 't14', 'checking', '', '', 'n', NULL, -900, 100, -900, 100, NULL),
    ('sta', 'tt', 'tmpl', '', '', 'n', NULL, 100, 100, 100, 100, NULL),
    ('stb', 'tt', 'tmpl', '', '', 'n', NULL, -100, 100, -100, 100, NULL);
-- slot_type: 1 int64, 4 string, 5 timespec, 6 guid, 9 frame.
INSERT INTO slots (obj_guid, name, slot_type, int64_val, string_val, timespec_val, guid_val) VALUES
    ('t1', 'notes', 4, NULL, 'Starting balances', NULL, NULL),
    ('t2', 'trans-txn-type', 4, NULL, 'I', NULL, NULL),
    ('t2', 'trans-date-due', 5, NULL, NULL, '2024-02-04 10:59:00', NULL),
    ('t3', 'trans-txn-type', 4, NULL, 'P', NULL, NULL),
    ('t4', 'trans-txn-type', 4, NULL, 'L', NULL, NULL),
    ('t5', 'trans-txn-type', 4, NULL, 'I', NULL, NULL),
    ('t6', 'trans-txn-type', 4, NULL, 'P', NULL, NULL),
    ('l1', 'gncInvoice', 9, NULL, NULL, NULL, 'f1'),
    ('f1', 'gncInvoice/invoice-guid', 6, NULL, NULL, NULL, 'inv1'),
    ('l1', 'gncOwner', 9, NULL, NULL, NULL, 'f2'),
    ('f2', 'gncOwner/owner-type', 1, 3, NULL, NULL, NULL),
    ('f2', 'gncOwner/owner-guid', 6, NULL, NULL, NULL, 'website'),
    ('l2', 'gncOwner', 9, NULL, NULL, NULL, 'f3'),
    ('f3', 'gncOwner/owner-type', 1, 2, NULL, NULL, NULL),
    ('f3', 'gncOwner/owner-guid', 6, NULL, NULL, NULL, 'acme'),
    ('l3', 'gncInvoice', 9, NULL, NULL, NULL, 'f4'),
    ('f4', 'gncInvoice/invoice-guid', 6, NULL, NULL, NULL, 'bill1'),
    ('l3', 'gncOwner', 9, NULL, NULL, NULL, 'f5'),
    ('f5', 'gncOwner/owner-type', 1, 4, NULL, NULL, NULL),
    ('f5', 'gncOwner/owner-guid', 6, NULL, NULL, NULL, 'paper');
INSERT INTO lots VALUES ('l1', 'ar', 0), ('l2', 'ar', 1), ('l3', 'ap', 1);
INSERT INTO customers (guid, name, id, notes, active, discount_num, discount_denom, credit_num, credit_denom, currency, tax_override,
        addr_name, addr_addr1, addr_addr2, addr_phone, addr_email, terms) VALUES
    ('acme', 'Acme Corp', '000001', 'Pays late', 1, 0, 1, 0, 1, 'usd', 0, 'Acme Corporation', '1 Main St', 'Springfield', '555-0100', 'ap@acme.test', 'net30');
INSERT INTO vendors (guid, name, id, notes, currency, active, tax_override, addr_name, addr_addr1) VALUES
    ('paper', 'Paper Co', '000001', '', 'usd', 1, 0, 'Paper Co', '2 Mill Rd');
INSERT INTO jobs VALUES ('website', '000001', 'Website', '', 1, 2, 'acme');
INSERT INTO invoices (guid, id, notes, active, currency, owner_type, owner_guid, terms, post_txn, post_lot, post_acc) VALUES
    ('inv1', '000001', '', 1, 'usd', 3, 'website', 'net30', 't2', 'l1', 'ar'),
    ('bill1', 'B-1', '', 1, 'usd', 4, 'paper', NULL, 't5', 'l3', 'ap'),
    ('draft', '000002', '', 1, 'usd', 2, 'acme', NULL, NULL, NULL, NULL);
INSERT INTO entries (guid, date, description, quantity_num, quantity_denom, i_acct, i_price_num, i_price_denom,
        i_discount_num, i_discount_denom, invoice, i_disc_type, b_acct, b_price_num, b_price_denom, bill) VALUES
    ('e1', '2024-01-05 10:59:00', 'Consulting hours', 2, 1, 'consulting', 5000, 100, 0, 1, 'inv1', 'PERCENT', NULL, 0, 1, NULL),
    ('e2', '2024-01-10 10:59:00', 'Printer paper', 3, 1, NULL, 0, 1, 0, 1, NULL, NULL, 'office', 1000, 100, 'bill1'),
    ('e3', '2024-01-11 10:59:00', 'Draft line', 1, 1, 'consulting', 999, 100, 0, 1, 'draft', 'PERCENT', NULL, 0, 1, NULL);
INSERT INTO billterms VALUES ('net30', 'Net 30', '', 1, 0, NULL, 'GNC_TERM_TYPE_DAYS', 30, 0, 0, 1, 0);
";

/// The same book in GnuCash's XML format, as GnuCash 5 writes it (trimmed of fields not read).
const XML: &str = r#"<?xml version="1.0" encoding="utf-8" ?>
<gnc-v2
     xmlns:gnc="http://www.gnucash.org/XML/gnc"
     xmlns:act="http://www.gnucash.org/XML/act"
     xmlns:book="http://www.gnucash.org/XML/book"
     xmlns:cd="http://www.gnucash.org/XML/cd"
     xmlns:cmdty="http://www.gnucash.org/XML/cmdty"
     xmlns:price="http://www.gnucash.org/XML/price"
     xmlns:slot="http://www.gnucash.org/XML/slot"
     xmlns:split="http://www.gnucash.org/XML/split"
     xmlns:trn="http://www.gnucash.org/XML/trn"
     xmlns:ts="http://www.gnucash.org/XML/ts"
     xmlns:lot="http://www.gnucash.org/XML/lot"
     xmlns:addr="http://www.gnucash.org/XML/addr"
     xmlns:billterm="http://www.gnucash.org/XML/billterm"
     xmlns:bt-days="http://www.gnucash.org/XML/bt-days"
     xmlns:cust="http://www.gnucash.org/XML/cust"
     xmlns:entry="http://www.gnucash.org/XML/entry"
     xmlns:invoice="http://www.gnucash.org/XML/invoice"
     xmlns:job="http://www.gnucash.org/XML/job"
     xmlns:owner="http://www.gnucash.org/XML/owner"
     xmlns:vendor="http://www.gnucash.org/XML/vendor">
<gnc:count-data cd:type="book">1</gnc:count-data>
<gnc:book version="2.0.0">
<book:id type="guid">book</book:id>
<gnc:count-data cd:type="commodity">3</gnc:count-data>
<gnc:commodity version="2.0.0">
  <cmdty:space>NASDAQ</cmdty:space>
  <cmdty:id>AAPL</cmdty:id>
  <cmdty:name>Apple Inc.</cmdty:name>
  <cmdty:fraction>10000</cmdty:fraction>
</gnc:commodity>
<gnc:pricedb version="1">
  <price>
    <price:commodity><cmdty:space>NASDAQ</cmdty:space><cmdty:id>AAPL</cmdty:id></price:commodity>
    <price:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></price:currency>
    <price:time><ts:date>2024-02-07 10:59:00 +0000</ts:date></price:time>
    <price:value>25000/100</price:value>
  </price>
</gnc:pricedb>
<gnc:account version="2.0.0">
  <act:name>Root Account</act:name>
  <act:id type="guid">root</act:id>
  <act:type>ROOT</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:commodity-scu>100</act:commodity-scu>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Assets</act:name>
  <act:id type="guid">assets</act:id>
  <act:type>ASSET</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:slots>
    <slot><slot:key>placeholder</slot:key><slot:value type="string">true</slot:value></slot>
  </act:slots>
  <act:parent type="guid">root</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Checking</act:name>
  <act:id type="guid">checking</act:id>
  <act:type>BANK</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:code>1010</act:code>
  <act:parent type="guid">assets</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Euro Account</act:name>
  <act:id type="guid">euro</act:id>
  <act:type>BANK</act:type>
  <act:commodity><cmdty:space>ISO4217</cmdty:space><cmdty:id>EUR</cmdty:id></act:commodity>
  <act:parent type="guid">assets</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Brokerage</act:name>
  <act:id type="guid">brokerage</act:id>
  <act:type>STOCK</act:type>
  <act:commodity><cmdty:space>NASDAQ</cmdty:space><cmdty:id>AAPL</cmdty:id></act:commodity>
  <act:parent type="guid">assets</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Accounts Receivable</act:name>
  <act:id type="guid">ar</act:id>
  <act:type>RECEIVABLE</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:parent type="guid">root</act:parent>
  <act:lots>
    <gnc:lot version="2.0.0">
      <lot:id type="guid">l1</lot:id>
      <lot:slots>
        <slot>
          <slot:key>gncInvoice</slot:key>
          <slot:value type="frame">
            <slot><slot:key>invoice-guid</slot:key><slot:value type="guid">inv1</slot:value></slot>
          </slot:value>
        </slot>
        <slot>
          <slot:key>gncOwner</slot:key>
          <slot:value type="frame">
            <slot><slot:key>owner-type</slot:key><slot:value type="integer">3</slot:value></slot>
            <slot><slot:key>owner-guid</slot:key><slot:value type="guid">website</slot:value></slot>
          </slot:value>
        </slot>
      </lot:slots>
    </gnc:lot>
    <gnc:lot version="2.0.0">
      <lot:id type="guid">l2</lot:id>
      <lot:slots>
        <slot>
          <slot:key>gncOwner</slot:key>
          <slot:value type="frame">
            <slot><slot:key>owner-type</slot:key><slot:value type="integer">2</slot:value></slot>
            <slot><slot:key>owner-guid</slot:key><slot:value type="guid">acme</slot:value></slot>
          </slot:value>
        </slot>
      </lot:slots>
    </gnc:lot>
  </act:lots>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Accounts Payable</act:name>
  <act:id type="guid">ap</act:id>
  <act:type>PAYABLE</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:parent type="guid">root</act:parent>
  <act:lots>
    <gnc:lot version="2.0.0">
      <lot:id type="guid">l3</lot:id>
      <lot:slots>
        <slot>
          <slot:key>gncInvoice</slot:key>
          <slot:value type="frame">
            <slot><slot:key>invoice-guid</slot:key><slot:value type="guid">bill1</slot:value></slot>
          </slot:value>
        </slot>
        <slot>
          <slot:key>gncOwner</slot:key>
          <slot:value type="frame">
            <slot><slot:key>owner-type</slot:key><slot:value type="integer">4</slot:value></slot>
            <slot><slot:key>owner-guid</slot:key><slot:value type="guid">paper</slot:value></slot>
          </slot:value>
        </slot>
      </lot:slots>
    </gnc:lot>
  </act:lots>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Liabilities</act:name>
  <act:id type="guid">liab</act:id>
  <act:type>LIABILITY</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:slots><slot><slot:key>placeholder</slot:key><slot:value type="string">true</slot:value></slot></act:slots>
  <act:parent type="guid">root</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Visa</act:name>
  <act:id type="guid">visa</act:id>
  <act:type>CREDIT</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:parent type="guid">liab</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Income</act:name>
  <act:id type="guid">income</act:id>
  <act:type>INCOME</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:slots><slot><slot:key>placeholder</slot:key><slot:value type="string">true</slot:value></slot></act:slots>
  <act:parent type="guid">root</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Consulting</act:name>
  <act:id type="guid">consulting</act:id>
  <act:type>INCOME</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:parent type="guid">income</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Expenses</act:name>
  <act:id type="guid">expenses</act:id>
  <act:type>EXPENSE</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:slots><slot><slot:key>placeholder</slot:key><slot:value type="string">true</slot:value></slot></act:slots>
  <act:parent type="guid">root</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Office</act:name>
  <act:id type="guid">office</act:id>
  <act:type>EXPENSE</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:slots><slot><slot:key>hidden</slot:key><slot:value type="string">true</slot:value></slot></act:slots>
  <act:parent type="guid">expenses</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Equity</act:name>
  <act:id type="guid">equity</act:id>
  <act:type>EQUITY</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:slots><slot><slot:key>placeholder</slot:key><slot:value type="string">true</slot:value></slot></act:slots>
  <act:parent type="guid">root</act:parent>
</gnc:account>
<gnc:account version="2.0.0">
  <act:name>Opening Balances</act:name>
  <act:id type="guid">opening</act:id>
  <act:type>EQUITY</act:type>
  <act:commodity><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></act:commodity>
  <act:parent type="guid">equity</act:parent>
</gnc:account>
<gnc:count-data cd:type="transaction">14</gnc:count-data>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t1</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-01-01 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:date-entered><ts:date>2024-01-01 10:59:00 +0000</ts:date></trn:date-entered>
  <trn:description>Opening balance</trn:description>
  <trn:slots>
    <slot><slot:key>date-posted</slot:key><slot:value type="gdate"><gdate>2024-01-01</gdate></slot:value></slot>
    <slot><slot:key>notes</slot:key><slot:value type="string">Starting balances</slot:value></slot>
  </trn:slots>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s1a</split:id>
      <split:reconciled-state>n</split:reconciled-state>
      <split:value>100000/100</split:value>
      <split:quantity>100000/100</split:quantity>
      <split:account type="guid">checking</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s1b</split:id>
      <split:reconciled-state>n</split:reconciled-state>
      <split:value>-100000/100</split:value>
      <split:quantity>-100000/100</split:quantity>
      <split:account type="guid">opening</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t2</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:num>000001</trn:num>
  <trn:date-posted><ts:date>2024-01-05 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Acme Corp</trn:description>
  <trn:slots>
    <slot>
      <slot:key>gncInvoice</slot:key>
      <slot:value type="frame">
        <slot><slot:key>invoice-guid</slot:key><slot:value type="guid">inv1</slot:value></slot>
      </slot:value>
    </slot>
    <slot><slot:key>trans-txn-type</slot:key><slot:value type="string">I</slot:value></slot>
  </trn:slots>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s2a</split:id>
      <split:reconciled-state>n</split:reconciled-state>
      <split:value>10000/100</split:value>
      <split:quantity>10000/100</split:quantity>
      <split:account type="guid">ar</split:account>
      <split:lot type="guid">l1</split:lot>
    </trn:split>
    <trn:split>
      <split:id type="guid">s2b</split:id>
      <split:memo>Consulting hours</split:memo>
      <split:reconciled-state>n</split:reconciled-state>
      <split:value>-10000/100</split:value>
      <split:quantity>-10000/100</split:quantity>
      <split:account type="guid">consulting</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t3</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-01-20 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Acme payment</trn:description>
  <trn:slots><slot><slot:key>trans-txn-type</slot:key><slot:value type="string">P</slot:value></slot></trn:slots>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s3a</split:id>
      <split:value>6000/100</split:value>
      <split:quantity>6000/100</split:quantity>
      <split:account type="guid">checking</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s3b</split:id>
      <split:value>-6000/100</split:value>
      <split:quantity>-6000/100</split:quantity>
      <split:account type="guid">ar</split:account>
      <split:lot type="guid">l2</split:lot>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t4</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-01-20 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Link</trn:description>
  <trn:slots><slot><slot:key>trans-txn-type</slot:key><slot:value type="string">L</slot:value></slot></trn:slots>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s4a</split:id>
      <split:value>6000/100</split:value>
      <split:quantity>6000/100</split:quantity>
      <split:account type="guid">ar</split:account>
      <split:lot type="guid">l2</split:lot>
    </trn:split>
    <trn:split>
      <split:id type="guid">s4b</split:id>
      <split:value>-6000/100</split:value>
      <split:quantity>-6000/100</split:quantity>
      <split:account type="guid">ar</split:account>
      <split:lot type="guid">l1</split:lot>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t5</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:num>B-1</trn:num>
  <trn:date-posted><ts:date>2024-01-10 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Paper Co</trn:description>
  <trn:slots><slot><slot:key>trans-txn-type</slot:key><slot:value type="string">I</slot:value></slot></trn:slots>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s5a</split:id>
      <split:memo>Printer paper</split:memo>
      <split:value>3000/100</split:value>
      <split:quantity>3000/100</split:quantity>
      <split:account type="guid">office</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s5b</split:id>
      <split:value>-3000/100</split:value>
      <split:quantity>-3000/100</split:quantity>
      <split:account type="guid">ap</split:account>
      <split:lot type="guid">l3</split:lot>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t6</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-01-25 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Pay Paper Co</trn:description>
  <trn:slots><slot><slot:key>trans-txn-type</slot:key><slot:value type="string">P</slot:value></slot></trn:slots>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s6a</split:id>
      <split:value>3000/100</split:value>
      <split:quantity>3000/100</split:quantity>
      <split:account type="guid">ap</split:account>
      <split:lot type="guid">l3</split:lot>
    </trn:split>
    <trn:split>
      <split:id type="guid">s6b</split:id>
      <split:value>-3000/100</split:value>
      <split:quantity>-3000/100</split:quantity>
      <split:account type="guid">checking</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t7</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:num>101</trn:num>
  <trn:date-posted><ts:date>2024-02-01 00:00:00 +0100</ts:date></trn:date-posted>
  <trn:description>Staples</trn:description>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s7a</split:id>
      <split:memo>Paper</split:memo>
      <split:value>1234/100</split:value>
      <split:quantity>1234/100</split:quantity>
      <split:account type="guid">office</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s7b</split:id>
      <split:memo/>
      <split:value>-1234/100</split:value>
      <split:quantity>-1234/100</split:quantity>
      <split:account type="guid">checking</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t8</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-02-02 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Coffee shop</trn:description>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s8a</split:id>
      <split:value>2000/100</split:value>
      <split:quantity>2000/100</split:quantity>
      <split:account type="guid">office</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s8b</split:id>
      <split:value>-2000/100</split:value>
      <split:quantity>-2000/100</split:quantity>
      <split:account type="guid">visa</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t9</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-02-03 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Pay the card</trn:description>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s9a</split:id>
      <split:value>2000/100</split:value>
      <split:quantity>2000/100</split:quantity>
      <split:account type="guid">visa</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s9b</split:id>
      <split:value>-2000/100</split:value>
      <split:quantity>-2000/100</split:quantity>
      <split:account type="guid">checking</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t10</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-02-04 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Fuel</trn:description>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s10a</split:id>
      <split:value>1005/1000</split:value>
      <split:quantity>1005/1000</split:quantity>
      <split:account type="guid">office</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s10b</split:id>
      <split:value>-1005/1000</split:value>
      <split:quantity>-1005/1000</split:quantity>
      <split:account type="guid">checking</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t11</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>EUR</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-02-05 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Paris client</trn:description>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s11a</split:id>
      <split:value>5000/100</split:value>
      <split:quantity>5000/100</split:quantity>
      <split:account type="guid">euro</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s11b</split:id>
      <split:value>-5000/100</split:value>
      <split:quantity>-5500/100</split:quantity>
      <split:account type="guid">consulting</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t12</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-02-06 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Fund the euro account</trn:description>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s12a</split:id>
      <split:value>11000/100</split:value>
      <split:quantity>10000/100</split:quantity>
      <split:account type="guid">euro</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s12b</split:id>
      <split:value>-11000/100</split:value>
      <split:quantity>-11000/100</split:quantity>
      <split:account type="guid">checking</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t13</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-02-07 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Buy AAPL</trn:description>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s13a</split:id>
      <split:action>Buy</split:action>
      <split:value>50000/100</split:value>
      <split:quantity>20000/10000</split:quantity>
      <split:account type="guid">brokerage</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s13b</split:id>
      <split:value>-50000/100</split:value>
      <split:quantity>-50000/100</split:quantity>
      <split:account type="guid">checking</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:transaction version="2.0.0">
  <trn:id type="guid">t14</trn:id>
  <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
  <trn:date-posted><ts:date>2024-02-08 10:59:00 +0000</ts:date></trn:date-posted>
  <trn:description>Typo</trn:description>
  <trn:splits>
    <trn:split>
      <split:id type="guid">s14a</split:id>
      <split:value>1000/100</split:value>
      <split:quantity>1000/100</split:quantity>
      <split:account type="guid">office</split:account>
    </trn:split>
    <trn:split>
      <split:id type="guid">s14b</split:id>
      <split:value>-900/100</split:value>
      <split:quantity>-900/100</split:quantity>
      <split:account type="guid">checking</split:account>
    </trn:split>
  </trn:splits>
</gnc:transaction>
<gnc:template-transactions>
  <gnc:account version="2.0.0">
    <act:name>Template Root</act:name>
    <act:id type="guid">troot</act:id>
    <act:type>ROOT</act:type>
  </gnc:account>
  <gnc:account version="2.0.0">
    <act:name>sx-account</act:name>
    <act:id type="guid">tmpl</act:id>
    <act:type>BANK</act:type>
    <act:parent type="guid">troot</act:parent>
  </gnc:account>
  <gnc:transaction version="2.0.0">
    <trn:id type="guid">tt</trn:id>
    <trn:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></trn:currency>
    <trn:date-posted><ts:date>2024-01-01 10:59:00 +0000</ts:date></trn:date-posted>
    <trn:description>Monthly rent</trn:description>
    <trn:splits>
      <trn:split><split:id type="guid">sta</split:id><split:value>100/100</split:value><split:quantity>100/100</split:quantity><split:account type="guid">tmpl</split:account></trn:split>
      <trn:split><split:id type="guid">stb</split:id><split:value>-100/100</split:value><split:quantity>-100/100</split:quantity><split:account type="guid">tmpl</split:account></trn:split>
    </trn:splits>
  </gnc:transaction>
</gnc:template-transactions>
<gnc:GncBillTerm version="2.0.0">
  <billterm:guid type="guid">net30</billterm:guid>
  <billterm:name>Net 30</billterm:name>
  <billterm:desc></billterm:desc>
  <billterm:refcount>1</billterm:refcount>
  <billterm:invisible>0</billterm:invisible>
  <billterm:days><bt-days:due-days>30</bt-days:due-days><bt-days:disc-days>0</bt-days:disc-days></billterm:days>
</gnc:GncBillTerm>
<gnc:GncCustomer version="2.0.0">
  <cust:guid type="guid">acme</cust:guid>
  <cust:name>Acme Corp</cust:name>
  <cust:id>000001</cust:id>
  <cust:addr version="2.0.0">
    <addr:name>Acme Corporation</addr:name>
    <addr:addr1>1 Main St</addr:addr1>
    <addr:addr2>Springfield</addr:addr2>
    <addr:phone>555-0100</addr:phone>
    <addr:email>ap@acme.test</addr:email>
  </cust:addr>
  <cust:notes>Pays late</cust:notes>
  <cust:terms type="guid">net30</cust:terms>
  <cust:active>1</cust:active>
  <cust:currency><cmdty:space>CURRENCY</cmdty:space><cmdty:id>USD</cmdty:id></cust:currency>
</gnc:GncCustomer>
<gnc:GncJob version="2.0.0">
  <job:guid type="guid">website</job:guid>
  <job:id>000001</job:id>
  <job:name>Website</job:name>
  <job:owner version="2.0.0"><owner:type>gncCustomer</owner:type><owner:id type="guid">acme</owner:id></job:owner>
  <job:active>1</job:active>
</gnc:GncJob>
<gnc:GncVendor version="2.0.0">
  <vendor:guid type="guid">paper</vendor:guid>
  <vendor:name>Paper Co</vendor:name>
  <vendor:id>000001</vendor:id>
  <vendor:addr version="2.0.0"><addr:name>Paper Co</addr:name><addr:addr1>2 Mill Rd</addr:addr1></vendor:addr>
  <vendor:active>1</vendor:active>
</gnc:GncVendor>
<gnc:GncInvoice version="2.0.0">
  <invoice:guid type="guid">inv1</invoice:guid>
  <invoice:id>000001</invoice:id>
  <invoice:owner version="2.0.0"><owner:type>gncJob</owner:type><owner:id type="guid">website</owner:id></invoice:owner>
  <invoice:opened><ts:date>2024-01-05 10:59:00 +0000</ts:date></invoice:opened>
  <invoice:posted><ts:date>2024-01-05 10:59:00 +0000</ts:date></invoice:posted>
  <invoice:terms type="guid">net30</invoice:terms>
  <invoice:active>1</invoice:active>
  <invoice:posttxn type="guid">t2</invoice:posttxn>
  <invoice:postlot type="guid">l1</invoice:postlot>
  <invoice:postacc type="guid">ar</invoice:postacc>
</gnc:GncInvoice>
<gnc:GncInvoice version="2.0.0">
  <invoice:guid type="guid">bill1</invoice:guid>
  <invoice:id>B-1</invoice:id>
  <invoice:owner version="2.0.0"><owner:type>gncVendor</owner:type><owner:id type="guid">paper</owner:id></invoice:owner>
  <invoice:active>1</invoice:active>
  <invoice:posttxn type="guid">t5</invoice:posttxn>
  <invoice:postlot type="guid">l3</invoice:postlot>
  <invoice:postacc type="guid">ap</invoice:postacc>
</gnc:GncInvoice>
<gnc:GncInvoice version="2.0.0">
  <invoice:guid type="guid">draft</invoice:guid>
  <invoice:id>000002</invoice:id>
  <invoice:owner version="2.0.0"><owner:type>gncCustomer</owner:type><owner:id type="guid">acme</owner:id></invoice:owner>
  <invoice:active>1</invoice:active>
</gnc:GncInvoice>
<gnc:GncEntry version="2.0.0">
  <entry:guid type="guid">e1</entry:guid>
  <entry:date><ts:date>2024-01-05 10:59:00 +0000</ts:date></entry:date>
  <entry:description>Consulting hours</entry:description>
  <entry:qty>2/1</entry:qty>
  <entry:i-acct type="guid">consulting</entry:i-acct>
  <entry:i-price>5000/100</entry:i-price>
  <entry:i-discount>0/1</entry:i-discount>
  <entry:invoice type="guid">inv1</entry:invoice>
  <entry:i-disc-type>PERCENT</entry:i-disc-type>
  <entry:i-disc-how>PRETAX</entry:i-disc-how>
  <entry:i-taxable>0</entry:i-taxable>
</gnc:GncEntry>
<gnc:GncEntry version="2.0.0">
  <entry:guid type="guid">e2</entry:guid>
  <entry:date><ts:date>2024-01-10 10:59:00 +0000</ts:date></entry:date>
  <entry:description>Printer paper</entry:description>
  <entry:qty>3/1</entry:qty>
  <entry:b-acct type="guid">office</entry:b-acct>
  <entry:b-price>1000/100</entry:b-price>
  <entry:bill type="guid">bill1</entry:bill>
</gnc:GncEntry>
<gnc:GncEntry version="2.0.0">
  <entry:guid type="guid">e3</entry:guid>
  <entry:date><ts:date>2024-01-11 10:59:00 +0000</ts:date></entry:date>
  <entry:description>Draft line</entry:description>
  <entry:qty>1/1</entry:qty>
  <entry:i-acct type="guid">consulting</entry:i-acct>
  <entry:i-price>999/100</entry:i-price>
  <entry:invoice type="guid">draft</entry:invoice>
</gnc:GncEntry>
</gnc:book>
</gnc-v2>
"#;

async fn sqlite_book() -> Book {
    let mut conn = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql(SCHEMA).execute(&mut conn).await.unwrap();
    sqlx::raw_sql(DATA).execute(&mut conn).await.unwrap();
    sqlite::read_connection(&mut conn).await.unwrap()
}

fn xml_book() -> Book {
    read_xml(XML.as_bytes()).unwrap()
}

fn by_name<'a>(ledger: &'a Ledger, name: &str) -> &'a SourceTransaction {
    ledger.transactions.iter().chain(&ledger.unbalanced).find(|t| t.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no transaction named {name}"))
}

fn account(ledger: &Ledger, full_name: &str) -> i64 {
    ledger.accounts.iter().find(|a| a.full_name == full_name).unwrap_or_else(|| panic!("no account {full_name}")).id
}

fn customer(ledger: &Ledger, name: &str) -> i64 {
    ledger.customers.iter().find(|c| c.name == name).unwrap().id
}

/// What the fixture book must read to, from either format.
fn check(ledger: &Ledger) {
    // Chart of accounts: no root, no stock account; paths keep the GnuCash tree, but a bank or
    // card account leaves a parent of another type.
    let names: Vec<&str> = ledger.accounts.iter().map(|a| a.full_name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Accounts Payable",
            "Accounts Receivable",
            "Assets",
            "Assets:Checking",
            "Assets:Euro Account",
            "Equity",
            "Equity:Opening Balances",
            "Expenses",
            "Expenses:Office",
            "Income",
            "Income:Consulting",
            "Liabilities",
            "Liabilities:Visa"
        ]
    );
    let get = |n: &str| ledger.accounts.iter().find(|a| a.full_name == n).unwrap();
    assert_eq!((get("Assets").account_type, get("Assets:Checking").account_type, get("Assets:Checking").parent_id), ("OTHER_CURRENT_ASSET", "BANK", None));
    assert_eq!(get("Liabilities:Visa").account_type, "CREDIT_CARD");
    assert_eq!(get("Expenses:Office").parent_id, Some(get("Expenses").id));
    assert!(!get("Expenses:Office").active, "hidden accounts import inactive");
    assert_eq!(get("Accounts Receivable").account_type, "ACCOUNTS_RECEIVABLE");

    // Lists: the job is a sub-customer.
    let customers: Vec<(&str, Option<i64>)> = ledger.customers.iter().map(|c| (c.name.as_str(), c.parent_id)).collect();
    let acme = customer(ledger, "Acme Corp");
    assert_eq!(customers, [("Acme Corp", None), ("Acme Corp:Website", Some(acme))]);
    let a = &ledger.customers[0];
    assert_eq!(
        (a.company_name.as_deref(), a.email.as_deref(), a.phone.as_deref(), a.address.as_deref(), a.terms_days, a.notes.as_deref()),
        (Some("Acme Corporation"), Some("ap@acme.test"), Some("555-0100"), Some("1 Main St\nSpringfield"), Some(30), Some("Pays late"))
    );
    assert_eq!(ledger.vendors.len(), 1);
    assert_eq!((ledger.vendors[0].name.as_str(), ledger.vendors[0].company_name.as_deref()), ("Paper Co", None));

    // Transactions: the link and the template are gone; the problems are listed.
    let kinds: Vec<(&str, &str)> = ledger.transactions.iter().map(|t| (t.kind, t.name.as_deref().unwrap_or(""))).collect();
    assert_eq!(
        kinds,
        [
            ("Opening balance", "Opening balance"),
            ("Invoice", "Acme Corp:Website"),
            ("Bill", "Paper Co"),
            ("Payment", "Acme Corp"),
            ("Bill payment", "Paper Co"),
            ("Check", "Staples"),
            ("Credit card charge", "Coffee shop"),
            ("Transfer", "Pay the card"),
            ("Transfer", "Fund the euro account"),
        ]
    );
    assert!(ledger.transactions.iter().all(|t| t.imbalance_cents() == 0));
    assert_eq!(ledger.unbalanced.len(), 1);
    assert_eq!((ledger.unbalanced[0].kind, ledger.unbalanced[0].imbalance_cents()), ("Withdrawal", -100));

    let opening = by_name(ledger, "Opening balance");
    assert_eq!(opening.memo.as_deref(), Some("Opening balance - Starting balances"));
    assert_eq!(opening.lines[0], SourceLine { account_id: account(ledger, "Assets:Checking"), cents: -100_000, memo: None });

    let invoice = by_name(ledger, "Acme Corp:Website");
    assert_eq!(invoice.date, NaiveDate::from_ymd_opt(2024, 1, 5).unwrap());
    let consulting = account(ledger, "Income:Consulting");
    assert_eq!(
        invoice.document,
        Some(SourceDocument::Invoice {
            customer_id: customer(ledger, "Acme Corp:Website"),
            number: Some("000001".into()),
            due_date: NaiveDate::from_ymd_opt(2024, 2, 4),
            lines: vec![DocumentLine {
                item_id: None,
                description: Some("Consulting hours".into()),
                quantity_e4: 20_000,
                rate_cents: 5_000,
                cents: 10_000,
                account_id: consulting
            }],
        })
    );
    let payment = by_name(ledger, "Acme Corp");
    let Some(SourceDocument::Payment { customer_id, applied_to, .. }) = &payment.document else { panic!("{payment:?}") };
    assert_eq!((*customer_id, applied_to.as_slice()), (acme, [Applied { transaction_id: invoice.id, cents: 6_000 }].as_slice()));

    let bill = ledger.transactions.iter().find(|t| t.kind == "Bill").unwrap();
    assert_eq!(bill.date, NaiveDate::from_ymd_opt(2024, 1, 10).unwrap(), "old-style SQLite dates read too");
    let Some(SourceDocument::Bill { lines, number, due_date, .. }) = &bill.document else { panic!("{bill:?}") };
    assert_eq!((number.as_deref(), *due_date, lines.len()), (Some("B-1"), None, 1));
    assert_eq!((lines[0].quantity_e4, lines[0].rate_cents, lines[0].cents), (30_000, 1_000, -3_000));
    let bill_payment = ledger.transactions.iter().find(|t| t.kind == "Bill payment").unwrap();
    let Some(SourceDocument::BillPayment { vendor_id, applied_to, .. }) = &bill_payment.document else { panic!() };
    assert_eq!((*vendor_id, applied_to.as_slice()), (Some(ledger.vendors[0].id), [Applied { transaction_id: bill.id, cents: 3_000 }].as_slice()));

    let check = by_name(ledger, "Staples");
    assert_eq!(check.date, NaiveDate::from_ymd_opt(2024, 2, 1).unwrap(), "local midnight stored as the evening before in UTC");
    assert_eq!(check.document, Some(SourceDocument::Expense { vendor_id: None, number: Some("101".into()), applied_to: Vec::new() }));

    // A home-currency transaction into the euro account carries its dollar value.
    let fund = by_name(ledger, "Fund the euro account");
    assert!(fund.lines.contains(&SourceLine { account_id: account(ledger, "Assets:Euro Account"), cents: -11_000, memo: None }));

    // Rejected: listed with the reason, never posted.
    assert_eq!(ledger.quality.rejected_lines, 3);
    assert_eq!(ledger.quality.rejected_by_reason.get(NOT_WHOLE_CENTS), Some(&1));
    assert_eq!(ledger.quality.rejected_by_reason.get(NO_HOME_AMOUNT), Some(&1));
    assert_eq!(ledger.quality.rejected_by_reason.get(SKIPPED_INVESTMENT), Some(&1));
    let listed: Vec<(&str, Option<i64>)> = ledger.unreadable.iter().map(|u| (u.name.as_deref().unwrap(), u.cents)).collect();
    assert_eq!(
        listed,
        [
            ("Fuel (1.005 is not a whole number of cents)", None),
            ("Fuel (1.005 is not a whole number of cents)", None),
            ("Paris client (in EUR with no USD amount)", None),
            ("Paris client (in EUR with no USD amount)", Some(5_500)),
            ("Buy AAPL (investment transaction: securities are not tracked)", Some(50_000)),
        ]
    );
    assert!(ledger.unreadable.iter().all(|u| u.outcome == LineOutcome::Unreadable));
    assert_eq!(ledger.quality.unreadable_lines, 5);
}

#[tokio::test]
async fn reads_a_sqlite_book() {
    check(&ledger(&sqlite_book().await));
}

#[test]
fn reads_an_xml_book() {
    check(&ledger(&xml_book()));
}

#[tokio::test]
async fn both_formats_read_the_same() {
    let (a, b) = (ledger(&sqlite_book().await), ledger(&xml_book()));
    assert_eq!(a.transactions, b.transactions);
    assert_eq!(a.unbalanced, b.unbalanced);
    assert_eq!(a.customers, b.customers);
    assert_eq!(a.vendors, b.vendors);
    let accounts = |l: &Ledger| l.accounts.iter().map(|a| (a.id, a.full_name.clone(), a.account_type, a.parent_id, a.active)).collect::<Vec<_>>();
    assert_eq!(accounts(&a), accounts(&b));
}

/// Files on disk, found by their first bytes: SQLite, gzip-compressed XML and plain XML.
#[tokio::test]
async fn reads_book_files_of_each_kind() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!("localbooks-gnucash-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let sqlite_path = dir.join("books.gnucash");
    let _ = std::fs::remove_file(&sqlite_path);
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&sqlite_path).create_if_missing(true);
    let mut conn = sqlx::ConnectOptions::connect(&options).await.unwrap();
    sqlx::raw_sql(SCHEMA).execute(&mut conn).await.unwrap();
    sqlx::raw_sql(DATA).execute(&mut conn).await.unwrap();
    conn.close().await.unwrap();

    let gz_path = dir.join("books-xml.gnucash");
    let mut gz = flate2::write::GzEncoder::new(std::fs::File::create(&gz_path).unwrap(), flate2::Compression::default());
    gz.write_all(XML.as_bytes()).unwrap();
    gz.finish().unwrap();
    let plain_path = dir.join("books.xml.gnucash");
    std::fs::write(&plain_path, XML).unwrap();

    for path in [&sqlite_path, &gz_path, &plain_path] {
        let ledger = read(path).await.unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        check(&ledger);
    }
    let modified = std::fs::metadata(&sqlite_path).unwrap().modified().unwrap();
    assert!(std::fs::read_dir(&dir).unwrap().count() == 3, "reading leaves no journal or lock files behind");
    assert_eq!(std::fs::metadata(&sqlite_path).unwrap().modified().unwrap(), modified);

    let other = dir.join("notes.gnucash");
    std::fs::write(&other, "just some text").unwrap();
    assert!(read(&other).await.unwrap_err().starts_with("GNUCASH_NOT_GNUCASH"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn rejects_xml_that_is_not_a_book() {
    assert!(read_xml(&b"<?xml version=\"1.0\"?><html><body/></html>"[..]).unwrap_err().starts_with("GNUCASH_NOT_GNUCASH"));
    assert!(read_xml(&b"<?xml version=\"1.0\"?><gnc-v2><gnc:book><gnc:account>"[..]).unwrap_err().starts_with("GNUCASH_PARSE_FAILED"), "a cut-off file");
}

#[test]
fn amounts_convert_exactly() {
    assert_eq!(Num::parse("-12345/100").and_then(Num::cents), Some(-12_345));
    assert_eq!(Num::parse("7").and_then(Num::cents), Some(700));
    assert_eq!(Num::parse("1/3").and_then(Num::cents), None);
    assert_eq!(Num::parse("1005/1000").and_then(Num::cents), None);
    assert_eq!(Num::parse("1000/1000").and_then(Num::cents), Some(100));
    assert_eq!(Num::parse("5/0"), None);
    assert_eq!(Num::new(1005, 1000).display(), "1.005");
    assert_eq!(Num::new(-5, 1000).display(), "-0.005");
    assert_eq!(Num::new(1, 3).display(), "1/3");
    assert_eq!(Num::rounded_cents(1005, 1000), Some(101), "half a cent rounds away from zero");
    assert_eq!(Num::rounded_cents(-1005, 1000), Some(-101));
}

#[test]
fn dates_read_in_every_stored_form() {
    let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day);
    assert_eq!(parse_date("2024-03-12 10:59:00"), d(2024, 3, 12));
    assert_eq!(parse_date("20240312105900"), d(2024, 3, 12));
    assert_eq!(parse_date("2024-03-12 05:00:00"), d(2024, 3, 12), "midnight in New York");
    assert_eq!(parse_date("2024-03-11 23:00:00"), d(2024, 3, 12), "midnight in Paris");
    assert_eq!(parse_date("2024-03-12 00:00:00 -0500"), d(2024, 3, 12));
    assert_eq!(parse_date("2024-03-12 10:59:00 +0000"), d(2024, 3, 12));
    assert_eq!(parse_date("2024-03-12"), d(2024, 3, 12));
    assert_eq!(parse_date("yesterday"), None);
}

/// An entry with a discount, and a tax line: the form keeps the entry at its net amount and the
/// tax as posted, and a form that does not add up falls back to the posted lines.
#[test]
fn invoice_lines_come_from_entries_only_when_they_add_up() {
    let mut book = xml_book();
    let consulting = "consulting".to_string();
    book.accounts.push(Account { guid: "tax".into(), name: "Sales Tax".into(), kind: "LIABILITY".into(), commodity: "CURRENCY:USD".into(), parent: Some("root".into()), ..Default::default() });
    // 2 × $50 less 10% = $90, plus $7.20 tax.
    let t2 = book.transactions.iter_mut().find(|t| t.guid == "t2").unwrap();
    t2.splits = vec![
        Split { account: "ar".into(), value: Num::new(9_720, 100), quantity: Num::new(9_720, 100), lot: Some("l1".into()), ..Default::default() },
        Split { account: consulting.clone(), value: Num::new(-9_000, 100), quantity: Num::new(-9_000, 100), ..Default::default() },
        Split { account: "tax".into(), value: Num::new(-720, 100), quantity: Num::new(-720, 100), memo: Some("Tax".into()), ..Default::default() },
    ];
    let e1 = book.entries.iter_mut().find(|e| e.invoice.as_deref() == Some("inv1")).unwrap();
    e1.i_discount = Num::new(10, 1);
    let l = ledger(&book);
    let invoice = l.transactions.iter().find(|t| t.kind == "Invoice").unwrap();
    let Some(SourceDocument::Invoice { lines, .. }) = &invoice.document else { panic!() };
    let got: Vec<(i64, i64, i64)> = lines.iter().map(|l| (l.quantity_e4, l.rate_cents, l.cents)).collect();
    assert_eq!(got, [(10_000, 9_000, 9_000), (10_000, 720, 720)]);
    assert_eq!(lines[0].description.as_deref(), Some("Consulting hours"));

    // The entry says $100 but the posting says $90: use the posting.
    let e1 = book.entries.iter_mut().find(|e| e.invoice.as_deref() == Some("inv1")).unwrap();
    e1.i_discount = Num::default();
    let l = ledger(&book);
    let invoice = l.transactions.iter().find(|t| t.kind == "Invoice").unwrap();
    let Some(SourceDocument::Invoice { lines, .. }) = &invoice.document else { panic!() };
    let got: Vec<(i64, Option<&str>)> = lines.iter().map(|l| (l.cents, l.description.as_deref())).collect();
    assert_eq!(got, [(9_000, None), (720, Some("Tax"))]);
}

/// A customer credit note applied to an invoice through a link transaction.
#[test]
fn credit_notes_apply_through_lot_links() {
    let mut book = xml_book();
    book.invoices.push(Invoice {
        guid: "cn1".into(),
        id: Some("CN-1".into()),
        owner: Some((OwnerKind::Customer, "acme".into())),
        post_txn: Some("tcn".into()),
        post_lot: Some("lcn".into()),
        credit_note: true,
        ..Default::default()
    });
    book.lots.insert("lcn".into(), Lot { owner: Some((OwnerKind::Customer, "acme".into())), invoice: Some("cn1".into()) });
    let usd = "CURRENCY:USD".to_string();
    let split = |account: &str, cents: i64, lot: Option<&str>| Split {
        account: account.into(),
        value: Num::new(cents, 100),
        quantity: Num::new(cents, 100),
        lot: lot.map(str::to_string),
        ..Default::default()
    };
    let date = NaiveDate::from_ymd_opt(2024, 1, 22);
    book.transactions.push(Transaction {
        guid: "tcn".into(),
        currency: usd.clone(),
        date,
        description: Some("Acme Corp".into()),
        txn_type: Some("I".into()),
        splits: vec![split("ar", -1_500, Some("lcn")), split("consulting", 1_500, None)],
        ..Default::default()
    });
    book.transactions.push(Transaction {
        guid: "tl2".into(),
        currency: usd,
        date,
        txn_type: Some("L".into()),
        splits: vec![split("ar", 1_500, Some("lcn")), split("ar", -1_500, Some("l1"))],
        ..Default::default()
    });
    let l = ledger(&book);
    let invoice = l.transactions.iter().find(|t| t.kind == "Invoice").unwrap();
    let credit = l.transactions.iter().find(|t| t.kind == "Credit memo").unwrap();
    let Some(SourceDocument::CreditMemo { applied_to, lines, .. }) = &credit.document else { panic!("{credit:?}") };
    assert_eq!(applied_to.as_slice(), [Applied { transaction_id: invoice.id, cents: 1_500 }]);
    assert_eq!(lines.iter().map(|l| l.cents).collect::<Vec<_>>(), [-1_500]);
    // The payment still applies its $60.
    let payment = l.transactions.iter().find(|t| t.kind == "Payment").unwrap();
    let Some(SourceDocument::Payment { applied_to, .. }) = &payment.document else { panic!() };
    assert_eq!(applied_to.iter().map(|a| a.cents).sum::<i64>(), 6_000);
}

/// A book whose transactions are mostly in euros makes euros the home currency: then a dollar
/// transaction reads only where its account is in euros.
#[test]
fn the_home_currency_is_the_one_most_transactions_use() {
    let mut book = xml_book();
    book.transactions.retain(|t| ["t11", "t12"].contains(&t.guid.as_str()));
    let mut second = book.transactions.iter().find(|t| t.guid == "t11").unwrap().clone();
    second.guid = "t11b".into();
    book.transactions.push(second);
    let l = ledger(&book);
    // Both euro transactions read by value; the dollar one has no euro amount on its checking split.
    assert_eq!(l.transactions.len(), 2);
    assert_eq!(l.quality.rejected_by_reason.get(NO_HOME_AMOUNT), Some(&1));
    assert_eq!(l.unreadable[0].name.as_deref(), Some("Fund the euro account (in USD with no EUR amount)"));
    let euro = account(&l, "Assets:Euro Account");
    assert!(l.transactions.iter().all(|t| t.lines.contains(&SourceLine { account_id: euro, cents: -5_000, memo: None })));
}

/// The fixture goes through staging and a full import: native invoice, payment, bill, bill
/// payment, check, card charge and transfers, with nothing falling back to a journal entry.
#[tokio::test]
async fn a_full_import_of_a_gnucash_book_creates_native_documents() {
    use crate::qbw_ledger_import::{auto_map_accounts, batch_summary, post_batch, stage, ImportMode, PostLedgerInput};
    let pool = crate::db::test_pool().await;
    let ledger = ledger(&sqlite_book().await);
    let batch = stage(&pool, Path::new("/books/Business.gnucash"), 1, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    let full = posted.full.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert!(summary.fallbacks.is_empty(), "{:?}", summary.fallbacks.iter().map(|f| (&f.kind, &f.reason)).collect::<Vec<_>>());
    assert_eq!(
        (full.invoices, full.payments, full.bills, full.bill_payments, full.checks, full.expenses, full.transfers, full.journal_entries),
        (1, 1, 1, 1, 1, 1, 2, 1),
        "{full:?}"
    );
    assert_eq!(summary.unbalanced.len(), 1);
    assert_eq!(summary.unreadable.len(), 5);

    let (status, total, paid): (String, i64, i64) = sqlx::query_as("SELECT status, total_cents, amount_paid_cents FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!((status.as_str(), total, paid), ("PARTIAL", 10_000, 6_000));
    let bill_status: String = sqlx::query_scalar("SELECT status FROM bills").fetch_one(&pool).await.unwrap();
    assert_eq!(bill_status, "PAID");
}

/// Writes the fixture book as `Sample SQLite.gnucash` and `Sample XML.gnucash` (compressed) into
/// LOCALBOOKS_GNUCASH_DIR, for trying the wizard by hand.
#[tokio::test]
#[ignore]
async fn write_sample_books() {
    use std::io::Write;
    let dir = std::path::PathBuf::from(std::env::var("LOCALBOOKS_GNUCASH_DIR").expect("set LOCALBOOKS_GNUCASH_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let sqlite_path = dir.join("Sample SQLite.gnucash");
    let _ = std::fs::remove_file(&sqlite_path);
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&sqlite_path).create_if_missing(true);
    let mut conn = sqlx::ConnectOptions::connect(&options).await.unwrap();
    sqlx::raw_sql(SCHEMA).execute(&mut conn).await.unwrap();
    sqlx::raw_sql(DATA).execute(&mut conn).await.unwrap();
    conn.close().await.unwrap();
    let mut gz = flate2::write::GzEncoder::new(std::fs::File::create(dir.join("Sample XML.gnucash")).unwrap(), flate2::Compression::default());
    gz.write_all(XML.as_bytes()).unwrap();
    gz.finish().unwrap();
}
