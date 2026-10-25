import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api, errorText } from '../lib/api.js';
import { askConfirm } from '../lib/dialogs.jsx';

const size = (bytes) => (bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`);

/** Files kept with a posted transaction (receipts, signed estimates): attach, save a copy, remove. */
export default function Attachments({ entryId }) {
  const [list, setList] = useState(null);
  const [error, setError] = useState(null);
  const load = () => api.listAttachments(entryId).then(setList, (err) => setError(errorText(err)));
  useEffect(() => {
    load();
  }, [entryId]); // eslint-disable-line react-hooks/exhaustive-deps

  const run = async (work) => {
    setError(null);
    try {
      await work();
      await load();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const attach = () =>
    run(async () => {
      const picked = await open({ multiple: true, directory: false });
      for (const path of [picked].flat().filter(Boolean)) await api.addAttachment(entryId, path);
    });

  return (
    <div className="flex flex-wrap items-center gap-2 px-2 py-1 pl-8 text-sm">
      <span className="text-qb-ink-muted">Attachments:</span>
      {list?.length === 0 && <span className="text-qb-ink-faint">none</span>}
      {list?.map((a) => (
        <span key={a.id} className="flex items-center gap-1 rounded-sm border border-qb-border bg-white px-1.5">
          <button type="button" className="text-qb-link hover:underline" onClick={() => run(() => api.saveAttachment(a.id))} title="Save a copy">
            {a.fileName}
          </button>
          <span className="text-2xs text-qb-ink-muted">{size(a.sizeBytes)}</span>
          <button
            type="button"
            className="text-qb-ink-muted hover:text-qb-negative"
            aria-label={`Remove ${a.fileName}`}
            onClick={async () => (await askConfirm(`Remove ${a.fileName} from this transaction?`, { confirmLabel: 'Remove', danger: true })) && run(() => api.deleteAttachment(a.id))}
          >
            ×
          </button>
        </span>
      ))}
      <button type="button" className="text-qb-link hover:underline" onClick={attach}>
        Attach file…
      </button>
      {error && <span className="text-qb-negative">{error}</span>}
    </div>
  );
}
