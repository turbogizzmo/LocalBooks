// The Other Names list: people and businesses who are neither customers nor vendors.
import { invoke } from '@tauri-apps/api/core';

export default {
  listOtherNames: () => invoke('list_other_names'),
  getOtherName: (id) => invoke('get_other_name', { id }),
  // input: { name, companyName?, email?, phone?, address?, notes? }
  createOtherName: (input) => invoke('create_other_name', { input }),
  updateOtherName: (id, input) => invoke('update_other_name', { id, input }),
  setOtherNameActive: (id, active) => invoke('set_other_name_active', { id, active }),
};
