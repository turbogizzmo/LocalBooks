// Condense Data: archive closed years and continue in a smaller company file.
import { invoke } from '@tauri-apps/api/core';

export default {
  previewCondense: (cutoff) => invoke('preview_condense', { cutoff }),
  condenseCompanyFile: (cutoff) => invoke('condense_company_file', { cutoff }),
  getCondenseStatus: () => invoke('get_condense_status'),
};
