// One screen of the app, in a window inside LocalBooks (see WindowManager.jsx): the page `view`,
// opened with `intent` (a request for an entry form, a record or a report, as `{ action, data, nonce }`).
import AllSales from './AllSales.jsx';
import AuditLog from './AuditLog.jsx';
import BankFeed from './BankFeed.jsx';
import Bills from './Bills.jsx';
import Budgets from './Budgets.jsx';
import ChartOfAccounts from './ChartOfAccounts.jsx';
import Classes from './Classes.jsx';
import CleanUpBooks from './CleanUpBooks.jsx';
import CreditMemos from './CreditMemos.jsx';
import Dashboard from './Dashboard.jsx';
import DepositsTransfers from './DepositsTransfers.jsx';
import Estimates from './Estimates.jsx';
import SalesOrders from './SalesOrders.jsx';
import ExpensesChecks from './ExpensesChecks.jsx';
import FinanceCharges from './FinanceCharges.jsx';
import BillCosts from './BillCosts.jsx';
import FindTransactions from './FindTransactions.jsx';
import FxRevaluation from './FxRevaluation.jsx';
import Invoices from './Invoices.jsx';
import JournalEntries from './JournalEntries.jsx';
import PartyList from './PartyList.jsx';
import Products from './Products.jsx';
import { PrintChecks, PrintInvoices } from './PrintForms.jsx';
import { MailingLabels, PrintBatch } from './PrintBatches.jsx';
import DocumentInbox from './DocumentInbox.jsx';
import BouncedChecks from './BouncedChecks.jsx';
import PurchaseOrders from './PurchaseOrders.jsx';
import Approvals from './Approvals.jsx';
import Reclassify from './Reclassify.jsx';
import Reconcile from './Reconcile.jsx';
import Recurring from './Recurring.jsx';
import Register from './Register.jsx';
import Reports from './Reports.jsx';
import SalesLists from './SalesLists.jsx';
import ProfileLists from './ProfileLists.jsx';
import SalesTax from './SalesTax.jsx';
import SendForms from './SendForms.jsx';
import Settings from './Settings.jsx';
import { canOpen, openableInstead } from './Sidebar.jsx';
import Statements from './Statements.jsx';
import TimeTracking from './TimeTracking.jsx';
import MileageTracking from './MileageTracking.jsx';
import Todos from './Todos.jsx';
import VendorCredits from './VendorCredits.jsx';
import AttachmentsLibrary from './AttachmentsLibrary.jsx';

export default function Screen({ view: chosenView, intent, navigate, accounts, isAdmin, companyName }) {
  // An Admin-only Settings tab shows anyone else their first tab instead (App also moves the window there).
  const view = canOpen(chosenView, isAdmin) ? chosenView : openableInstead(chosenView);
  const list = accounts;
  const props = { accounts: list, intent, navigate };
  return (
    <>
      {view === 'dashboard' && <Dashboard accounts={list} navigate={navigate} />}
      {view === 'sales/all' && <AllSales {...props} />}
      {view === 'sales/invoices' && <Invoices {...props} />}
      {view === 'sales/estimates' && <Estimates {...props} />}
      {view === 'sales/orders' && <SalesOrders {...props} />}
      {view === 'sales/credits' && <CreditMemos {...props} />}
      {view === 'sales/customers' && <PartyList kind="customer" navigate={navigate} intent={intent} />}
      {view === 'sales/products' && <Products accounts={list} intent={intent} />}
      {view === 'sales/lists' && <SalesLists />}
      {view === 'sales/profile-lists' && <ProfileLists intent={intent} />}
      {view === 'sales/tax' && <SalesTax accounts={list} intent={intent} />}
      {view === 'sales/statements' && <Statements intent={intent} />}
      {view === 'sales/finance-charges' && <FinanceCharges accounts={list} navigate={navigate} />}
      {view === 'sales/bill-costs' && <BillCosts navigate={navigate} />}
      {view === 'sales/send-forms' && <SendForms />}
      {view === 'sales/print-invoices' && <PrintInvoices />}
      {view === 'sales/print-forms' && <PrintBatch key={intent?.action ?? 'credit-memos'} action={intent?.action} accounts={list} />}
      {view === 'sales/mailing-labels' && <MailingLabels />}
      {view === 'expenses/print-checks' && <PrintChecks accounts={list} />}
      {view === 'expenses/inbox' && <DocumentInbox />}
      {view === 'expenses/expenses' && <ExpensesChecks {...props} />}
      {view === 'expenses/bills' && <Bills {...props} />}
      {view === 'expenses/credits' && <VendorCredits {...props} />}
      {view === 'expenses/purchase-orders' && <PurchaseOrders {...props} />}
      {view === 'expenses/approvals' && <Approvals isAdmin={isAdmin !== false} />}
      {view === 'expenses/vendors' && <PartyList kind="vendor" navigate={navigate} />}
      {view === 'employees/time' && <TimeTracking {...props} />}
      {view === 'employees/mileage' && <MileageTracking {...props} />}
      {view === 'banking/feeds' && <BankFeed accounts={list} />}
      {view === 'banking/transfers' && <DepositsTransfers {...props} />}
      {view === 'banking/reconcile' && <Reconcile accounts={list} />}
      {view === 'banking/bounced-checks' && <BouncedChecks accounts={list} />}
      {view === 'accounting/chart-of-accounts' && <ChartOfAccounts accounts={list} intent={intent} navigate={navigate} />}
      {view === 'accounting/classes' && <Classes />}
      {view === 'accounting/reclassify' && <Reclassify accounts={list} />}
      {view === 'accounting/clean-up' && <CleanUpBooks accounts={list} />}
      {view === 'accounting/register' && <Register accounts={list} intent={intent} />}
      {view === 'accounting/journal' && <JournalEntries {...props} />}
      {view === 'accounting/find' && <FindTransactions {...props} />}
      {view === 'accounting/recurring' && <Recurring />}
      {view === 'accounting/todos' && <Todos {...props} />}
      {view === 'accounting/budgets' && <Budgets accounts={list} navigate={navigate} />}
      {view === 'accounting/audit' && <AuditLog />}
      {view === 'accounting/attachments' && <AttachmentsLibrary />}
      {view === 'accounting/fx-revaluation' && <FxRevaluation />}
      {view === 'reports' && <Reports navigate={navigate} intent={intent} />}
      {view.startsWith('settings/') && (
        <Settings accounts={list} companyName={companyName} intent={intent} isAdmin={isAdmin} navigate={navigate} tab={view.slice('settings/'.length)} />
      )}
    </>
  );
}
