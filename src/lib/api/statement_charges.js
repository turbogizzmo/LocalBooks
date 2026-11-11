// Customers › Enter Statement Charges: a customer's register of charges billed on the next statement.
import { invoke } from '@tauri-apps/api/core';
import { saveNamed } from '../confirmable.js';

export default {
  listStatementCharges: (customerId) => invoke('list_statement_charges', { customerId }),
  // input: { customerName, chargeDate, dueDate?, productServiceId?, description, quantityE4?, rateCents, incomeAccountId, classId? }
  createStatementCharge: (input) => saveNamed('create_statement_charge', { input }),
};
