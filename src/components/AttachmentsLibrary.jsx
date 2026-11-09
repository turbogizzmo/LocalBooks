import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatDate } from '../lib/dates.js';
import { askConfirm } from '../lib/dialogs.jsx';
import { attachedLabel, fileSize } from '../lib/attachmentsLibrary.js';
import { useOpenFromList } from '../lib/windows.js';

/**
 * Company › Attachments Library (T58): every file kept in the company file, on transactions and on
 * customers, vendors, items, accounts, estimates, purchase orders and reconciliations. Search by
 * file name, keywords or what it is attached to; click one to preview it (pictures and text), set
 * its keywords and whether it goes with the form when emailed, save a copy or delete it.
 *
 * Deleting is reversible (R9): a deleted file moves to Show deleted, where it can be restored, or
 * removed for good after a warning (Admins only).
 */
export default function AttachmentsLibrary() {
  const [query, setQuery] = useState('');
  const [showDeleted, setShowDeleted] = useState(false);
  const [selected, setSelected] = useState(null); // `${source}-${id}`
  const [notice, setNotice] = useState(null);
  const { data, error, reload } = useLedgerQuery(() => api.listAttachmentLibrary(query.trim() || null, showDeleted));
  useEffect(() => {
    reload();
  }, [query, showDeleted, reload]);
  const files = data ?? [];
  const keyOf = (f) => `${f.source}-${f.id}`;
  const current = files.find((f) => keyOf(f) === selected) ?? null;

  return (
    <div className="flex h-full gap-2 p-2">
      <section className="qb-panel flex min-h-0 min-w-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Attachments Library</h2>
          <input
            className="qb-input h-row w-[260px] text-sm"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search file names, keywords, names"
            aria-label="Search attachments"
          />
          <label className="flex items-center gap-1 text-sm">
            <input type="checkbox" className="accent-qb-green" checked={showDeleted} onChange={(e) => setShowDeleted(e.target.checked)} />
            Show deleted
          </label>
          <span className="ml-auto text-xs text-qb-ink-muted">
            {files.length} file{files.length === 1 ? '' : 's'}
          </span>
        </div>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {error && <Notice kind="error" text={errorText(error)} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head">Added</th>
                <th className="qb-grid-head">File</th>
                <th className="qb-grid-head">Keywords</th>
                <th className="qb-grid-head">Attached to</th>
                <th className="qb-grid-head text-right">Size</th>
                <th className="qb-grid-head" title="Sent with the form when it is emailed">
                  With form
                </th>
              </tr>
            </thead>
            <tbody>
              {data && files.length === 0 && (
                <tr>
                  <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">
                    {query.trim() ? 'No file matches.' : 'No files yet. Attach them on a transaction, customer, vendor, item, account, estimate, purchase order or reconciliation.'}
                  </td>
                </tr>
              )}
              {files.map((f) => (
                <tr
                  key={keyOf(f)}
                  className={`qb-grid-row cursor-pointer ${keyOf(f) === selected ? 'is-active' : ''} ${f.deletedAt ? 'text-qb-ink-faint' : ''}`}
                  aria-selected={keyOf(f) === selected}
                  onClick={() => setSelected(keyOf(f))}
                >
                  <td className="qb-grid-cell">{formatDate(f.createdAt.slice(0, 10))}</td>
                  <td className="qb-grid-cell truncate" title={f.fileName}>
                    {f.fileName}
                    {f.deletedAt && <span className="ml-1 text-2xs font-semibold uppercase text-qb-negative">Deleted</span>}
                  </td>
                  <td className="qb-grid-cell truncate text-qb-ink-muted">{f.keywords ?? ''}</td>
                  <td className="qb-grid-cell truncate">{attachedLabel(f, SOURCE_LABEL)}</td>
                  <td className="qb-grid-cell num">{fileSize(f.sizeBytes)}</td>
                  <td className="qb-grid-cell text-center">{f.sendWithForm ? '✓' : ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
      {current && <FilePanel key={keyOf(current)} file={current} onNotice={setNotice} onChanged={reload} />}
    </div>
  );
}

/** The chosen file: its preview, details and what can be done with it. */
function FilePanel({ file, onNotice, onChanged }) {
  const ref = { source: file.source, id: file.id };
  const [preview, setPreview] = useState(null);
  const [keywords, setKeywords] = useState(file.keywords ?? '');
  const [withForm, setWithForm] = useState(file.sendWithForm);
  const openFromList = useOpenFromList();
  // Only pictures and text are previewed; other files aren't requested at all.
  const previewable = file.mediaType.startsWith('image/') || file.mediaType.startsWith('text/');
  useEffect(() => {
    let live = true;
    if (!previewable) {
      setPreview({ mediaType: file.mediaType, dataBase64: null });
      return undefined;
    }
    api.getAttachmentPreview(ref).then(
      (p) => live && setPreview(p),
      () => live && setPreview({ dataBase64: null }),
    );
    return () => {
      live = false;
    };
  }, [file.source, file.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const run = async (work, success) => {
    try {
      await work();
      if (success) onNotice({ kind: 'success', text: success });
      onChanged();
    } catch (err) {
      onNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const changed = keywords.trim() !== (file.keywords ?? '') || withForm !== file.sendWithForm;
  const goTo = () => {
    if (file.source === 'ENTRY') openFromList('accounting/journal', 'open-entry', { entryId: file.attachedId });
  };

  return (
    <aside className="qb-panel flex w-[320px] flex-none flex-col overflow-hidden" aria-label={`${file.fileName}: details`}>
      <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
        <h2 className="truncate text-md font-semibold" title={file.fileName}>
          {file.fileName}
        </h2>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto p-2 text-sm scrollbar-thin">
        <div className="flex min-h-[160px] items-center justify-center rounded-sm border border-qb-border bg-qb-canvas p-1">
          {!preview && <span className="text-qb-ink-muted">Loading…</span>}
          {preview?.dataBase64 && preview.mediaType.startsWith('image/') && (
            <img src={`data:${preview.mediaType};base64,${preview.dataBase64}`} alt={file.fileName} className="max-h-[320px] max-w-full object-contain" />
          )}
          {preview?.dataBase64 && preview.mediaType.startsWith('text/') && (
            <pre className="max-h-[320px] w-full overflow-auto whitespace-pre-wrap text-xs">{decodeText(preview.dataBase64)}</pre>
          )}
          {preview && !(preview.dataBase64 && (preview.mediaType.startsWith('image/') || preview.mediaType.startsWith('text/'))) && (
            <span className="px-2 text-center text-qb-ink-muted">No preview for this kind of file here. Save a copy to open it.</span>
          )}
        </div>
        <dl className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
          <dt className="text-qb-ink-muted">Attached to</dt>
          <dd>
            {file.source === 'ENTRY' ? (
              <button type="button" className="text-left text-qb-link hover:underline" onClick={goTo}>
                {attachedLabel(file, SOURCE_LABEL)}
              </button>
            ) : (
              attachedLabel(file, SOURCE_LABEL)
            )}
          </dd>
          <dt className="text-qb-ink-muted">Added</dt>
          <dd>{formatDate(file.createdAt.slice(0, 10))}</dd>
          <dt className="text-qb-ink-muted">Size</dt>
          <dd>{fileSize(file.sizeBytes)}</dd>
          {file.deletedAt && (
            <>
              <dt className="text-qb-negative">Deleted</dt>
              <dd className="text-qb-negative">{formatDate(file.deletedAt.slice(0, 10))}</dd>
            </>
          )}
        </dl>
        {!file.deletedAt && (
          <>
            <label className="flex flex-col gap-0.5">
              <span className="qb-label">Keywords</span>
              <input className="qb-input" value={keywords} onChange={(e) => setKeywords(e.target.value)} maxLength={500} placeholder="e.g. W-9 2025, signed" />
            </label>
            <label className="flex items-center gap-1" title="Sent along with the invoice, estimate or sales receipt when it is emailed">
              <input type="checkbox" className="accent-qb-green" checked={withForm} onChange={(e) => setWithForm(e.target.checked)} />
              Send with the form when it is emailed
            </label>
            <button type="button" className="qb-btn qb-btn-primary h-row self-start px-2" disabled={!changed} onClick={() => run(() => api.setAttachmentDetails(ref, keywords.trim() || null, withForm), 'Saved.')}>
              Save details
            </button>
          </>
        )}
        <div className="flex flex-wrap gap-1 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={() => run(() => api.saveLibraryAttachment(ref))}>
            Save a copy…
          </button>
          {file.deletedAt ? (
            <>
              <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={() => run(() => api.restoreAttachment(ref), `${file.fileName} is back with what it was attached to.`)}>
                Restore
              </button>
              <button
                type="button"
                className="qb-btn qb-btn-secondary h-row px-2 text-qb-negative"
                onClick={async () => {
                  const ok = await askConfirm(`Remove ${file.fileName} for good? It can't be restored afterwards. Keep records for as long as your tax authority asks (often several years).`, {
                    confirmLabel: 'Remove for good',
                    danger: true,
                  });
                  if (ok) run(() => api.purgeAttachment(ref), `${file.fileName} was removed for good.`);
                }}
              >
                Remove for good…
              </button>
            </>
          ) : (
            <button
              type="button"
              className="qb-btn qb-btn-secondary h-row px-2"
              onClick={async () => {
                if (await askConfirm(`Delete ${file.fileName}? It moves to Show deleted, where you can restore it.`, { confirmLabel: 'Delete', danger: true })) {
                  run(() => api.deleteLibraryAttachment(ref), `${file.fileName} deleted. Tick Show deleted to restore it.`);
                }
              }}
            >
              Delete…
            </button>
          )}
        </div>
      </div>
    </aside>
  );
}

function decodeText(base64) {
  try {
    const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return '';
  }
}
