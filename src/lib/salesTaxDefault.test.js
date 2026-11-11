import { test } from 'node:test';
import assert from 'node:assert/strict';
import { companyDefaultRateId, startingTaxRateId } from './salesTaxDefault.js';

const rates = [{ id: 1 }, { id: 2 }];

test('a customer on Company default gets the company default rate when there are several rates', () => {
  assert.equal(startingTaxRateId({ taxRateId: null, isTaxExempt: false }, rates, 2), 2);
  assert.equal(startingTaxRateId(null, rates, 2), 2, 'a new name too');
});

test('the customer’s own rate and exemption win over the company default', () => {
  assert.equal(startingTaxRateId({ taxRateId: 1, isTaxExempt: false }, rates, 2), 1);
  assert.equal(startingTaxRateId({ taxRateId: 1, isTaxExempt: true }, rates, 2), null);
});

test('without a usable default: the only rate, else no tax', () => {
  assert.equal(startingTaxRateId({ taxRateId: null }, [{ id: 7 }], null), 7);
  assert.equal(startingTaxRateId({ taxRateId: null }, rates, null), null);
  assert.equal(startingTaxRateId({ taxRateId: null }, rates, 9), null, 'a default that is no longer active');
});

test('the preference reads as an id or nothing', () => {
  assert.equal(companyDefaultRateId({ sales_tax_default_rate_id: '4' }), 4);
  assert.equal(companyDefaultRateId({ sales_tax_default_rate_id: '' }), null);
  assert.equal(companyDefaultRateId({}), null);
  assert.equal(companyDefaultRateId(undefined), null);
});
