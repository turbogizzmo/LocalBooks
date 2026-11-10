// Closing Date Exceptions: postings and voids made in a closed period.
import { invoke } from '@tauri-apps/api/core';

export default {
  getClosingExceptions: () => invoke('get_closing_exceptions'),
};
