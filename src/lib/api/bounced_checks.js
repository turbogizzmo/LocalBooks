// Record Bounced Check.
import { invoke } from '@tauri-apps/api/core';

export default {
  listBounceCandidates: () => invoke('list_bounce_candidates'),
  listBouncedChecks: () => invoke('list_bounced_checks'),
  recordBouncedCheck: (input) => invoke('record_bounced_check', { input }),
  voidBouncedCheck: (id) => invoke('void_bounced_check', { id }),
};
