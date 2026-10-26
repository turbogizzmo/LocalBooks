// Sales reps and shipping methods.
import { invoke } from '@tauri-apps/api/core';

export default {
  listSalesReps: () => invoke('list_sales_reps'),
  createSalesRep: (input) => invoke('create_sales_rep', { input }),
  updateSalesRep: (id, input) => invoke('update_sales_rep', { id, input }),
  setSalesRepActive: (id, active) => invoke('set_sales_rep_active', { id, active }),
  deleteSalesRep: (id) => invoke('delete_sales_rep', { id }),
  listShipMethods: () => invoke('list_ship_methods'),
  createShipMethod: (name) => invoke('create_ship_method', { name }),
  renameShipMethod: (id, name) => invoke('rename_ship_method', { id, name }),
  setShipMethodActive: (id, active) => invoke('set_ship_method_active', { id, active }),
  deleteShipMethod: (id) => invoke('delete_ship_method', { id }),
};
