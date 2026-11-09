// Company Snapshot: the page of widgets.
import { invoke } from '@tauri-apps/api/core';

export default {
  getCompanySnapshot: () => invoke('get_company_snapshot'),
  addSnapshotWidget: (kind) => invoke('add_snapshot_widget', { kind }),
  removeSnapshotWidget: (id) => invoke('remove_snapshot_widget', { id }),
  setSnapshotWidgetPeriod: (id, period) => invoke('set_snapshot_widget_period', { id, period }),
  arrangeSnapshotWidgets: (order) => invoke('arrange_snapshot_widgets', { order }),
};
