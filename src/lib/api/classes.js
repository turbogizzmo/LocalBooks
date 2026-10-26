// Classes and reclassifying lines.
import { invoke } from '@tauri-apps/api/core';

export default {
  listClasses: () => invoke('list_classes'),
  createClass: (input) => invoke('create_class', { input }),
  updateClass: (id, input) => invoke('update_class', { id, input }),
  setClassActive: (id, active) => invoke('set_class_active', { id, active }),
  deleteClass: (id) => invoke('delete_class', { id }),
  listReclassLines: (query) => invoke('list_reclass_lines', { query }),
  reclassifyLines: (lineIds, classId) => invoke('reclassify_lines', { lineIds, classId }),
};
