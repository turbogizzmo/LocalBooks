// Employees and time.
import { invoke } from '@tauri-apps/api/core';

export default {
  listEmployees: () => invoke('list_employees'),
  createEmployee: (name, email) => invoke('create_employee', { name, email: email || null }),
  createTimeActivities: (inputs) => invoke('create_time_activities', { inputs }),
  listTimeActivities: (from, to) => invoke('list_time_activities', { from, to }),
};
