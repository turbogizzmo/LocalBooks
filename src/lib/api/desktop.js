// Windows inside LocalBooks and the saved desktop: what each LocalBooks window has open, Cascade and Tile, Desktop View preferences.
import { invoke } from '@tauri-apps/api/core';

export default {
  reportChildWindows: (children, active) => invoke('report_child_windows', { children, active }),
  arrangeWindows: (arrangement, count, area) => invoke('arrange_windows', { arrangement, count, area }),
  takeDesktop: (knownViews) => invoke('take_desktop', { knownViews }),
  getDesktopView: () => invoke('get_desktop_view'),
  setDesktopView: (choice) => invoke('set_desktop_view', { choice }),
  setWindowPreferences: ({ windowMode = null, showOpenWindowList = null } = {}) => invoke('set_window_preferences', { windowMode, showOpenWindowList }),
};
