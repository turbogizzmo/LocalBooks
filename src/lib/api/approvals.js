// Approvals for bills and purchase orders.
import { invoke } from '@tauri-apps/api/core';

export default {
  listApprovalRules: () => invoke('list_approval_rules'),
  addApprovalRule: (input) => invoke('add_approval_rule', { input }),
  deleteApprovalRule: (id) => invoke('delete_approval_rule', { id }),
  submitForApproval: (kind, input) => invoke('submit_for_approval', { kind, input }),
  // Create Bill on a purchase order when an approval rule holds the bill; approving it bills the order.
  submitPurchaseOrderBillForApproval: (id, billDate, dueDate, billNumber) => invoke('submit_purchase_order_bill_for_approval', { id, billDate, dueDate, billNumber }),
  listApprovalRequests: (status = null) => invoke('list_approval_requests', { status }),
  approveRequest: (id) => invoke('approve_request', { id }),
  rejectRequest: (id, note) => invoke('reject_request', { id, note }),
};
