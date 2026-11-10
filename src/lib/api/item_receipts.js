// Item receipts: Receive Items and Enter Bill for Received Items (T12).
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  receiveItems: (input) => recordTransaction('receive_items', { input }),
  listItemReceipts: (openOnly = false) => invoke('list_item_receipts', { openOnly }),
  listItemReceiptLines: (itemReceiptId) => invoke('list_item_receipt_lines', { itemReceiptId }),
  listOrderReceiving: (purchaseOrderId) => invoke('list_order_receiving', { purchaseOrderId }),
  getReceiptBillFormLines: (itemReceiptId) => invoke('get_receipt_bill_form_lines', { itemReceiptId }),
  billReceivedItems: (input) => recordTransaction('bill_received_items', { input }),
};
