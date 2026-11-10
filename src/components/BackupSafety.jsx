// Settings › File & Data: whether a backup is safe from this disk failing (copied off it) and has
// been shown to restore (a test restore), with Test restore… for any backup file.
import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import Notice from './Notice.jsx';
import { Group } from './PreferencesWindow.jsx';

const when = (t) => (t ? t.replace('T', ' ').slice(0, 16) : 'never');

/** What a passed test restore found, in words. */
export function testRestoreText(info) {
  const name = info.companyName ? `${info.companyName}'s backup` : 'The backup';
  const last = info.lastEntryDate ? `, the latest dated ${info.lastEntryDate}` : '';
  const upgrade = info.needsUpgrade ? ' It is from an older version and would be upgraded when restored.' : '';
  return `Test restore passed: ${name} opens and checks out, with ${info.journalEntries} journal entries${last}. Your books were not changed.${upgrade} (Only a backup of these books counts toward the Backup safety reminder.)`;
}

/** `onTest(path)` is shared with the snapshot list's Test restore buttons. */
export function useTestRestore(onNotice) {
  const [busy, setBusy] = useState(false);
  const test = async (path) => {
    setBusy(true);
    try {
      onNotice({ kind: 'success', text: testRestoreText(await api.testRestoreBackup(path)) });
    } catch (err) {
      onNotice({ kind: 'error', text: `Test restore failed: ${errorText(err)}. Make a new backup and test that one.` });
    } finally {
      setBusy(false);
    }
  };
  return [test, busy];
}

export default function BackupSafety() {
  const { data } = useLedgerQuery(api.getBackupSafety);
  const [notice, setNotice] = useState(null);
  const [test, busy] = useTestRestore(setNotice);
  const choose = async () => {
    const path = await open({ multiple: false, directory: false, filters: [{ name: 'LocalBooks backup', extensions: ['localbooks', 'db'] }] });
    if (typeof path === 'string') await test(path);
  };
  return (
    <Group title="Backup safety">
      <p className="text-sm">
        Last copy off this computer: <b>{when(data?.lastOffDisk)}</b> · Last test restore: <b>{when(data?.lastTestRestore)}</b>
      </p>
      <p className="text-xs text-qb-ink-muted">
        A copy counts as off this computer when it is saved with Back up to… on another drive, a network share or a synced folder, or made by the offsite
        backup. A test restore opens a backup and checks it as a restore would, without replacing your books. Reminders lists either when it is overdue
        (Edit › Preferences › Reminders sets how many days).
      </p>
      <div className="flex gap-2">
        <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={choose} disabled={busy}>
          {busy ? 'Testing…' : 'Test restore a backup…'}
        </button>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
    </Group>
  );
}
