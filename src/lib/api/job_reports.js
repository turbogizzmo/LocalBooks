// Job costing reports: profitability, estimates against actuals and Profit and Loss by Job.
import { invoke } from '@tauri-apps/api/core';

export default {
  getJobReport: (kind, params) => invoke('get_job_report', { kind, params }),
  getProfitAndLossByJob: (startDate, endDate) => invoke('get_profit_and_loss_by_job', { startDate, endDate }),
};
