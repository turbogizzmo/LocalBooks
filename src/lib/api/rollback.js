// Help › Reinstall Previous Version (gap R13): the release before this one, through the signed updater.
import { invoke } from '@tauri-apps/api/core';

export default {
  /** { currentVersion, previous: { version, tag, publishedAt } | null, blocked: reason | null } */
  getPreviousRelease: () => invoke('get_previous_release'),
  /** Backs up the open books, installs the previous release (signature checked); a message to show. */
  reinstallPreviousRelease: () => invoke('reinstall_previous_release'),
};
