// Add/Edit Multiple Items (Products & Services): rows pasted from a spreadsheet, as tab-separated
// cells in a fixed column order.

/** The pasted columns, in order: [field, label]. */
export const PASTE_COLUMNS = [
  ['name', 'Name'],
  ['description', 'Description'],
  ['rate', 'Price'],
  ['incomeAccount', 'Income account'],
  ['purchaseDescription', 'Purchase description'],
  ['cost', 'Cost'],
];

/** Rows pasted from a spreadsheet: tab-separated cells; a first row of column names is skipped. */
export function parsePastedItems(text) {
  const rows = text
    .split(/\r?\n/)
    .filter((l) => l.trim() !== '')
    .map((l) => l.split('\t').map((c) => c.trim()));
  if (rows.length > 0 && rows[0][0]?.toLowerCase() === 'name') rows.shift();
  return rows.map((cells) => Object.fromEntries(PASTE_COLUMNS.map(([id], i) => [id, cells[i] ?? ''])));
}
