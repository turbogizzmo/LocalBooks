// 1099 report.
import { invoke } from '@tauri-apps/api/core';

export default {
  get1099Report: (year) => invoke('get_1099_report', { year }),
};
