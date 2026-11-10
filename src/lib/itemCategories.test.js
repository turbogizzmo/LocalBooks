import { test } from 'node:test';
import assert from 'node:assert/strict';
import { categoryIndex } from './itemCategories.js';

const categories = [
  { id: 1, name: 'Tools', parentId: null },
  { id: 2, name: 'Power', parentId: 1 },
  { id: 3, name: 'Cordless', parentId: 2 },
  { id: 4, name: 'Paint', parentId: null },
];
const items = [
  { id: 10, categoryId: 3 },
  { id: 11, categoryId: 1 },
  { id: 12, categoryId: 4 },
  { id: 13, categoryId: null },
];

test('a category covers its sub-categories, Uncategorized only items without one', () => {
  const index = categoryIndex(categories);
  const under = (chosen) => items.filter((i) => index.covers(chosen, i.categoryId)).map((i) => i.id);
  assert.deepEqual(under(undefined), [10, 11, 12, 13]);
  assert.deepEqual(under(1), [10, 11]);
  assert.deepEqual(under(2), [10]);
  assert.deepEqual(under(4), [12]);
  assert.deepEqual(under(null), [13]);
});

test('counts include sub-categories', () => {
  const counts = categoryIndex(categories).counts(items);
  assert.equal(counts.all, 4);
  assert.equal(counts.none, 1);
  assert.deepEqual([...counts.byCategory], [[1, 2], [2, 1], [3, 1], [4, 1]]);
});
