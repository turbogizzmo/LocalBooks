// Third-party notices.
import { invoke } from '@tauri-apps/api/core';

export default {
  getThirdPartyNotices: () => invoke('get_third_party_notices'),
};
