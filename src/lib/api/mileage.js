// Mileage: vehicles, mileage rates, trips, billing trips and the mileage reports.
import { invoke } from '@tauri-apps/api/core';

export default {
  listVehicles: () => invoke('list_vehicles'),
  saveVehicle: (id, input) => invoke('save_vehicle', { id: id ?? null, input }),
  listMileageRates: () => invoke('list_mileage_rates'),
  saveMileageRate: (id, input) => invoke('save_mileage_rate', { id: id ?? null, input }),
  deleteMileageRate: (id) => invoke('delete_mileage_rate', { id }),
  listMileageTrips: (from, to) => invoke('list_mileage_trips', { from, to }),
  listUnbilledMileage: (customerId) => invoke('list_unbilled_mileage', { customerId: customerId ?? null }),
  createMileageTrip: (input) => invoke('create_mileage_trip', { input }),
  updateMileageTrip: (id, input) => invoke('update_mileage_trip', { id, input }),
  deleteMileageTrip: (id) => invoke('delete_mileage_trip', { id }),
  billMileageTrips: (input) => invoke('bill_mileage_trips', { input }),
  getMileageReport: (kind, params) => invoke('get_mileage_report', { kind, params }),
};
