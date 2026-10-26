// File checkup, rebuild and forensic corrections.
import { invoke } from '@tauri-apps/api/core';

export default {
  fileDentistCheckup: () => invoke('file_dentist_checkup'),
  fileDentistRebuild: () => invoke('file_dentist_rebuild'),
  proposeForensicCorrections: () => invoke('propose_forensic_corrections'),
  applyForensicCorrection: (input) => invoke('apply_forensic_correction', { input }),
};
