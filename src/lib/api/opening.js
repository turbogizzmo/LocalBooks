// The company file opening right now, if any (gap R28): the window shows Opening <name>… until it ends.
import { invoke } from '@tauri-apps/api/core';

export default {
  getOpeningState: () => invoke('get_opening_state'),
};
