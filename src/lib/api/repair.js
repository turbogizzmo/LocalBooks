// File checkup, rebuild and forensic corrections.
import { invoke } from '@tauri-apps/api/core';

export default {
  fileRepairCheckup: () => invoke('file_repair_checkup'),
  fileRepairRebuild: () => invoke('file_repair_rebuild'),
  proposeForensicCorrections: () => invoke('propose_forensic_corrections'),
  applyForensicCorrection: (input) => invoke('apply_forensic_correction', { input }),
};
