// Cash Flow Forecast: cash on hand plus money due in and out, week by week.
import { invoke } from '@tauri-apps/api/core';

export default {
  getCashFlowForecast: (asOf, weeks) => invoke('get_cash_flow_forecast', { asOf, weeks }),
};
