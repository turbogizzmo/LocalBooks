// Windows and the saved desktop: what each window shows, the Open Window List, Desktop View preferences.
import { invoke } from '@tauri-apps/api/core';

export default {
  reportWindowPage: (view, action = null, data = null, content = null) => invoke('report_window_page', { view, action, data, content }),
  listOpenWindows: () => invoke('list_open_windows'),
  focusOpenWindow: (label) => invoke('focus_open_window', { label }),
  takeDesktop: (knownViews) => invoke('take_desktop', { knownViews }),
  getDesktopView: () => invoke('get_desktop_view'),
  setDesktopView: (choice) => invoke('set_desktop_view', { choice }),
  setWindowPreferences: ({ windowMode = null, showOpenWindowList = null } = {}) => invoke('set_window_preferences', { windowMode, showOpenWindowList }),
};
