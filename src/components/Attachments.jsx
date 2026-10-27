import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
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

/** Asks for files to attach, for a form that attaches them once it posts. Returns the chosen paths. */
export async function pickAttachments() {
  const picked = await open({ multiple: true, directory: false });
  return [picked].flat().filter(Boolean);
}

/** Attaches files picked on a form to the transaction it just posted. Returns an error message, or null. */
export async function attachPicked(entryId, paths) {
  try {
    for (const path of paths) await api.addAttachment(entryId, path);
    return null;
  } catch (err) {
    return `It posted, but a file could not be attached: ${errorText(err)}`;
  }
}

/** The files picked on a form before it posts, with a button to pick more and × to drop one. */
export function PendingAttachments({ paths, onChange, label = 'Attach file…' }) {
  const name = (path) => path.split(/[\\/]/).pop();
  return (
    <span className="flex flex-wrap items-center gap-1 text-sm">
      {paths.map((path) => (
        <span key={path} className="flex items-center gap-1 rounded-sm border border-qb-border bg-white px-1.5" title={path}>
          📎 {name(path)}
          <button type="button" className="text-qb-ink-muted hover:text-qb-negative" aria-label={`Don't attach ${name(path)}`} onClick={() => onChange(paths.filter((p) => p !== path))}>
            ×
          </button>
        </span>
      ))}
      <button type="button" className="text-qb-link hover:underline" onClick={async () => onChange([...new Set([...paths, ...(await pickAttachments())])])}>
        {label}
      </button>
    </span>
  );
}

/**
 * Attachments for a register: a paperclip on rows with files, an Attachments… row-menu item, and the
 * row under a transaction that lists its files once opened.
 */
export function useAttachmentRows() {
  const { data } = useLedgerQuery(api.attachmentCounts);
  const [openId, setOpenId] = useState(null);
  const counts = data ?? {};
  const toggle = (entryId) => setOpenId((id) => (id === entryId ? null : entryId));
  return {
    menuItem: (entryId) => ({
      label: counts[entryId] ? `Attachments (${counts[entryId]})…` : 'Attach file…',
      disabled: !entryId,
      onSelect: () => toggle(entryId),
    }),
    clip: (entryId) =>
      entryId && counts[entryId] ? (
        <button
          type="button"
          className="ml-1 text-2xs text-qb-ink-muted hover:text-qb-link"
          // Rows such as invoices act on mousedown, so stop that too, not just the click.
          onMouseDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation();
            toggle(entryId);
          }}
          title={`${counts[entryId]} attachment${counts[entryId] === 1 ? '' : 's'}`}
          aria-label={`Show ${counts[entryId]} attachment${counts[entryId] === 1 ? '' : 's'}`}
        >
          📎{counts[entryId] > 1 ? counts[entryId] : ''}
        </button>
      ) : null,
    row: (entryId, colSpan) =>
      entryId && entryId === openId ? (
        <tr>
          <td colSpan={colSpan} className="border-b border-qb-border bg-qb-canvas p-0">
            <Attachments entryId={entryId} />
          </td>
        </tr>
      ) : null,
  };
}
