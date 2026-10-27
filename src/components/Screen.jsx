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
import ExpensesChecks from './ExpensesChecks.jsx';
import FinanceCharges from './FinanceCharges.jsx';
import FindTransactions from './FindTransactions.jsx';
import FxRevaluation from './FxRevaluation.jsx';
import Invoices from './Invoices.jsx';
import JournalEntries from './JournalEntries.jsx';
import PartyList from './PartyList.jsx';
import Products from './Products.jsx';
import PurchaseOrders from './PurchaseOrders.jsx';
import Reclassify from './Reclassify.jsx';
import Reconcile from './Reconcile.jsx';
import Recurring from './Recurring.jsx';
import Register from './Register.jsx';
import Reports from './Reports.jsx';
import SalesLists from './SalesLists.jsx';
import SalesTax from './SalesTax.jsx';
import SendForms from './SendForms.jsx';
import Settings from './Settings.jsx';
import { canOpen, findNav } from './Sidebar.jsx';
import Statements from './Statements.jsx';
import TimeTracking from './TimeTracking.jsx';
import MileageTracking from './MileageTracking.jsx';
import Todos from './Todos.jsx';
import VendorCredits from './VendorCredits.jsx';

export default function Screen({ view: chosenView, intent, navigate, accounts, isAdmin, companyName }) {
  // An Admin-only Settings tab (left open, or restored with the desktop) shows anyone else their first tab instead.
  const view = canOpen(chosenView, isAdmin) ? chosenView : findNav('settings').item.children.find((page) => canOpen(page.id, false)).id;
  const list = accounts;
  const props = { accounts: list, intent, navigate };
  return (
    <>
      {view === 'dashboard' && <Dashboard navigate={navigate} />}
      {view === 'sales/all' && <AllSales {...props} />}
      {view === 'sales/invoices' && <Invoices {...props} />}
      {view === 'sales/estimates' && <Estimates {...props} />}
      {view === 'sales/credits' && <CreditMemos {...props} />}
      {view === 'sales/customers' && <PartyList kind="customer" navigate={navigate} />}
      {view === 'sales/products' && <Products accounts={list} intent={intent} />}
      {view === 'sales/lists' && <SalesLists />}
      {view === 'sales/tax' && <SalesTax accounts={list} />}
      {view === 'sales/statements' && <Statements />}
      {view === 'sales/finance-charges' && <FinanceCharges accounts={list} navigate={navigate} />}
      {view === 'sales/send-forms' && <SendForms />}
      {view === 'expenses/expenses' && <ExpensesChecks {...props} />}
      {view === 'expenses/bills' && <Bills {...props} />}
      {view === 'expenses/credits' && <VendorCredits {...props} />}
      {view === 'expenses/purchase-orders' && <PurchaseOrders {...props} />}
      {view === 'expenses/vendors' && <PartyList kind="vendor" navigate={navigate} />}
      {view === 'employees/time' && <TimeTracking {...props} />}
      {view === 'employees/mileage' && <MileageTracking {...props} />}
      {view === 'banking/feeds' && <BankFeed accounts={list} />}
      {view === 'banking/transfers' && <DepositsTransfers {...props} />}
      {view === 'banking/reconcile' && <Reconcile accounts={list} />}
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
      {view === 'accounting/fx-revaluation' && <FxRevaluation />}
      {view === 'reports' && <Reports navigate={navigate} intent={intent} />}
      {view.startsWith('settings/') && (
        <Settings accounts={list} companyName={companyName} intent={intent} isAdmin={isAdmin} navigate={navigate} tab={view.slice('settings/'.length)} />
      )}
    </>
  );
}
