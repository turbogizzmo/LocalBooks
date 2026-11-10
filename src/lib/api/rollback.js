// Help › Reinstall Previous Version (gap R13): the release before this one, through the signed updater.
import { invoke } from '@tauri-apps/api/core';

export default {
  /**
   * @returns {Promise<{ currentVersion: string, previous: { version: string, tag: string, publishedAt: string | null } | null, blocked: string | null }>}
   *   `blocked` says why the previous version can't open the open company file.
   */
  getPreviousRelease: () => invoke('get_previous_release'),
  /** Backs up the open books, installs the previous release (signature checked); a message to show. */
  reinstallPreviousRelease: () => invoke('reinstall_previous_release'),
};
