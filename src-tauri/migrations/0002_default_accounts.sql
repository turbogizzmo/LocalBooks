-- 0002_default_accounts.sql
-- Default small-business chart of accounts, numbered QuickBooks-style
-- (1xxx assets, 2xxx liabilities, 3xxx equity, 4xxx income, 5xxx COGS, 6xxx expenses, 7xxx/8xxx other).

UPDATE chart_of_accounts SET account_number = '1200' WHERE is_system = 1 AND name = 'Accounts Receivable (A/R)';
UPDATE chart_of_accounts SET account_number = '1300' WHERE is_system = 1 AND name = 'Undeposited Funds';
UPDATE chart_of_accounts SET account_number = '2000' WHERE is_system = 1 AND name = 'Accounts Payable (A/P)';
UPDATE chart_of_accounts SET account_number = '3000' WHERE is_system = 1 AND name = 'Opening Balance Equity';
UPDATE chart_of_accounts SET account_number = '3900' WHERE is_system = 1 AND name = 'Retained Earnings';

INSERT INTO chart_of_accounts (account_number, name, account_type, description) VALUES
    ('1010', 'Checking',                  'BANK',                    'Primary operating account'),
    ('1020', 'Savings',                   'BANK',                    'Reserve account'),
    ('1500', 'Equipment',                 'FIXED_ASSET',             'Computers, machinery, furniture'),
    ('2100', 'Credit Card',               'CREDIT_CARD',             'Business credit card'),
    ('2200', 'Sales Tax Payable',         'OTHER_CURRENT_LIABILITY', 'Sales tax collected, not yet remitted'),
    ('2500', 'Loan Payable',              'LONG_TERM_LIABILITY',     'Long-term business loans'),
    ('3100', 'Owner''s Investment',       'EQUITY',                  'Capital contributed by owners'),
    ('3200', 'Owner''s Draw',             'EQUITY',                  'Withdrawals by owners'),
    ('4000', 'Sales',                     'INCOME',                  'Product sales'),
    ('4100', 'Services',                  'INCOME',                  'Service revenue'),
    ('4900', 'Uncategorized Income',      'INCOME',                  'Deposits awaiting classification'),
    ('5000', 'Cost of Goods Sold',        'COST_OF_GOODS_SOLD',      'Direct cost of products sold'),
    ('6000', 'Advertising & Marketing',   'EXPENSE',                 NULL),
    ('6050', 'Bank Charges & Fees',       'EXPENSE',                 NULL),
    ('6100', 'Fuel & Auto',               'EXPENSE',                 NULL),
    ('6150', 'Insurance',                 'EXPENSE',                 NULL),
    ('6200', 'Meals & Entertainment',     'EXPENSE',                 NULL),
    ('6250', 'Office Supplies',           'EXPENSE',                 NULL),
    ('6300', 'Rent & Lease',              'EXPENSE',                 NULL),
    ('6350', 'Software & Cloud Services', 'EXPENSE',                 NULL),
    ('6400', 'Contractors',               'EXPENSE',                 NULL),
    ('6450', 'Travel',                    'EXPENSE',                 NULL),
    ('6500', 'Utilities',                 'EXPENSE',                 NULL),
    ('6550', 'Telephone & Internet',      'EXPENSE',                 NULL),
    ('6600', 'Shipping & Postage',        'EXPENSE',                 NULL),
    ('6900', 'Uncategorized Expense',     'EXPENSE',                 'Payments awaiting classification'),
    ('7000', 'Interest Income',           'OTHER_INCOME',            NULL),
    ('8000', 'Interest Expense',          'OTHER_EXPENSE',           NULL);
