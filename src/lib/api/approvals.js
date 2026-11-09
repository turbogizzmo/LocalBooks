// Approvals for bills and purchase orders.
import { invoke } from '@tauri-apps/api/core';

export default {
  listApprovalRules: () => invoke('list_approval_rules'),
  addApprovalRule: (input) => invoke('add_approval_rule', { input }),
  deleteApprovalRule: (id) => invoke('delete_approval_rule', { id }),
  submitForApproval: (kind, input) => invoke('submit_for_approval', { kind, input }),
  listApprovalRequests: (status = null) => invoke('list_approval_requests', { status }),
  approveRequest: (id) => invoke('approve_request', { id }),
  rejectRequest: (id, note) => invoke('reject_request', { id, note }),
};
