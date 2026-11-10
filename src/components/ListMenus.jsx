import RowMenu from './RowMenu.jsx';
import { useReportOpener } from '../lib/quickReport.js';
import { runPrintCommand } from '../lib/printCommand.js';
import { useOpenFromList } from '../lib/windows.js';

/**
 * The menus over the customer, vendor and item lists (gap T33): New Transactions (the forms for
 * the selected name; an invoice starts with the selected customer), Print (the list on screen, and
 * the list reports) and Excel (the list as a workbook or CSV, from `exports`).
 */
const NEW = {
  customer: [
    ['Invoice', 'sales/invoices', 'invoice'],
    ['Sales Receipt', 'sales/all', 'sales-receipt'],
    ['Estimate', 'sales/estimates', 'estimate'],
    ['Receive Payment', 'sales/invoices', 'receive-payment'],
    ['Credit Memo / Refund', 'sales/credits', 'credit-memo'],
  ],
  vendor: [
    ['Enter Bill', 'expenses/bills', 'bill'],
    ['Pay Bills', 'expenses/bills', 'pay-bills'],
    ['Write Check', 'expenses/expenses', 'check'],
    ['Vendor Credit', 'expenses/credits', 'vendor-credit'],
    ['Purchase Order', 'expenses/purchase-orders', 'purchase-order'],
  ],
  item: [
    ['Invoice', 'sales/invoices', 'invoice'],
    ['Sales Receipt', 'sales/all', 'sales-receipt'],
    ['Purchase Order', 'expenses/purchase-orders', 'purchase-order'],
  ],
};

const REPORTS = {
  customer: [
    ['Customer Phone List', 'list:customer-phone'],
    ['Customer Contact List', 'list:customer-contact'],
  ],
  vendor: [
    ['Vendor Phone List', 'list:vendor-phone'],
    ['Vendor Contact List', 'list:vendor-contact'],
  ],
  item: [
    ['Item Listing', 'list:item-listing'],
    ['Item Price List', 'item-price-list'],
  ],
};

export default function ListMenus({ kind, selected = null, exports }) {
  const openFromList = useOpenFromList();
  const openReport = useReportOpener();
  const newItems = NEW[kind].map(([label, view, action]) => ({
    label: `${label}…`,
    onSelect: () => openFromList(view, action, kind === 'customer' && selected ? { customerName: selected.name } : null),
  }));
  const printItems = [
    { label: 'Print this list…', onSelect: () => runPrintCommand() },
    ...REPORTS[kind].map(([label, report]) => ({ label, onSelect: () => openReport(['reports', 'open', { report }]) })),
  ];
  const excelItems = [
    { label: 'Export this list to Excel…', onSelect: exports.onXlsx },
    { label: 'Export this list as CSV…', onSelect: exports.onCsv },
  ];
  return (
    <>
      <RowMenu text="New Transactions" items={newItems} />
      <RowMenu text="Print" items={printItems} />
      <RowMenu text="Excel" items={excelItems} />
    </>
  );
}
