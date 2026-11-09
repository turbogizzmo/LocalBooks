// Merging two customers, vendors, items or accounts.
import { invoke } from '@tauri-apps/api/core';

export default {
  findMergeTarget: (kind, id, name) => invoke('find_merge_target', { kind, id, name }),
  mergeNames: (kind, fromId, toId) => invoke('merge_names', { kind, fromId, toId }),
};
