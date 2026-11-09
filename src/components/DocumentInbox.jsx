import { useEffect, useMemo, useState } from 'react';
import { contentMatches } from '../lib/inboxPreview.js';
import PdfPreview from './PdfPreview.jsx';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { useOpenFromList } from '../lib/windows.js';

const size = (bytes) => (bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`);
const canPreview = (media) => media === 'application/pdf' || media.startsWith('image/');

/** Bytes as a data URL, which the content security policy lets an <img> show (a blob: URL it doesn't). */
function dataUrl(mediaType, bytes) {
  let text = '';
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return `data:${mediaType};base64,${btoa(text)}`;
}

/** The selected document: a PDF drawn page by page (PdfPreview), or a picture. */
function Preview({ doc }) {
  const [url, setUrl] = useState(null);
  const [pdf, setPdf] = useState(null);
  const [error, setError] = useState(null);
  useEffect(() => {
    let live = true;
    setUrl(null);
    setPdf(null);
    setError(null);
    if (!canPreview(doc.mediaType)) return undefined;
    api.inboxDocumentContent(doc.id).then(
      (bytes) => {
        if (!live) return;
        // A PDF is previewed only when its bytes are a PDF: a file named .pdf that holds something
        // else (a web page, say) is never handed to the viewer. Images show in an <img>, which runs
        // nothing.
        if (doc.mediaType === 'application/pdf' && !contentMatches(doc.mediaType, new Uint8Array(bytes))) {
          setError('This file’s contents don’t match its name, so it isn’t previewed. It is attached as it is when you enter it.');
          return;
        }
        if (doc.mediaType === 'application/pdf') setPdf(new Uint8Array(bytes));
        else setUrl(dataUrl(doc.mediaType, new Uint8Array(bytes)));
      },
      (err) => live && setError(errorText(err)),
    );
    return () => {
      live = false;
    };
  }, [doc.id, doc.mediaType]);
  if (error) return <Notice kind="error" text={error} />;
  if (!canPreview(doc.mediaType)) return <p className="p-3 text-sm text-qb-ink-muted">No preview for this kind of file. It is attached as it is when you enter it.</p>;
  if (pdf) return <PdfPreview bytes={pdf} name={doc.fileName} className="h-full p-2" />;
  if (!url) return <p className="p-3 text-sm text-qb-ink-muted">Loading…</p>;
  return (
    <img alt={`Preview of ${doc.fileName}`} src={url} className="mx-auto max-h-full max-w-full object-contain" />
  );
}

/**
 * Document Inbox: receipts and bills waiting to be entered. Drop files on the window or click Add
 * files…; each is kept in the company file with a preview, and Enter as bill / expense / check opens
 * that form filled in from what was read from the file (a PDF's total, date and name), with the file
 * attached when it posts.
 */
export default function DocumentInbox() {
  const { data, error } = useLedgerQuery(api.listInboxDocuments);
  const docs = useMemo(() => data ?? [], [data]);
  const [selectedId, setSelectedId] = useState(null);
  const [notice, setNotice] = useState(null);
  const [hover, setHover] = useState(false);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState('');
  const openForm = useOpenFromList();
  const selected = docs.find((d) => d.id === selectedId) ?? null;

  useEffect(() => {
    if (!docs.some((d) => d.id === selectedId)) setSelectedId(docs[0]?.id ?? null);
  }, [docs, selectedId]);
  useEffect(() => setNote(selected?.note ?? ''), [selected?.id, selected?.note]); // eslint-disable-line react-hooks/exhaustive-deps

  const add = async (paths) => {
    if (!paths.length) return;
    setBusy(true);
    try {
      const added = await api.addInboxDocuments(paths);
      setSelectedId(added[added.length - 1]?.id ?? null);
      setNotice({ kind: 'success', text: `Added ${added.length} document${added.length === 1 ? '' : 's'} to the inbox.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  // Files dropped on the window come in with their paths (native drag and drop).
  useEffect(() => {
    let unlisten;
    let cancelled = false;
    try {
      getCurrentWebview()
        .onDragDropEvent((event) => {
          const { type, paths } = event.payload;
          if (type === 'enter' || type === 'over') setHover(true);
          else if (type === 'leave') setHover(false);
          else if (type === 'drop') {
            setHover(false);
            if (paths?.length) add(paths);
          }
        })
        .then((fn) => (cancelled ? fn() : (unlisten = fn)))
        .catch(() => {});
    } catch {
      // Not running inside a Tauri webview: Add files… still works.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const browse = async () => {
    const picked = await open({ multiple: true, directory: false, filters: [{ name: 'Receipts and bills', extensions: ['pdf', 'PDF', 'png', 'PNG', 'jpg', 'JPG', 'jpeg', 'JPEG', 'heic', 'HEIC', 'gif', 'webp'] }] });
    add([picked].flat().filter(Boolean));
  };
  const act = async (work, text) => {
    try {
      await work();
      if (text) setNotice({ kind: 'success', text });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  // The form opens in a window of its own with this document, read and attached.
  const enter = (view, action) => openForm(view, action, { inbox: selected });

  return (
    <div className={`flex h-full flex-col gap-2 p-2 ${hover ? 'outline outline-2 outline-qb-green' : ''}`}>
      <div className="qb-panel flex flex-none flex-wrap items-center gap-2 px-2 py-1">
        <h2 className="text-md font-semibold">Document Inbox</h2>
        <button type="button" className="qb-btn qb-btn-primary" onClick={browse} disabled={busy}>
          Add files…
        </button>
        <span className="text-xs text-qb-ink-muted">
          Or drop receipts and bills on this window. They're kept in the company file until you enter them; nothing is sent anywhere.
        </span>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load the inbox: ${error}`} />}
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(280px,2fr)_minmax(0,3fr)] gap-2">
        <section className="qb-panel flex min-h-0 flex-col" aria-label="Waiting to be entered">
          <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
            <h3 className="text-sm font-semibold">Waiting to be entered</h3>
            <span className="ml-2 text-xs text-qb-ink-muted">{docs.length}</span>
          </div>
          <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head">Document</th>
                  <th className="qb-grid-head">Read from it</th>
                  <th className="qb-grid-head text-right">Amount</th>
                </tr>
              </thead>
              <tbody>
                {data && docs.length === 0 && (
                  <tr>
                    <td colSpan={3} className="qb-grid-cell text-qb-ink-muted">
                      Nothing waiting. Add a receipt or a vendor's bill, then enter it as an expense, check or bill.
                    </td>
                  </tr>
                )}
                {docs.map((d) => (
                  <tr key={d.id} aria-selected={d.id === selectedId} className="qb-grid-row cursor-pointer" onMouseDown={() => setSelectedId(d.id)}>
                    <td className="qb-grid-cell" title={`${d.fileName} · ${size(d.sizeBytes)} · added ${formatDate(d.addedAt.slice(0, 10))}`}>
                      {d.fileName}
                      {d.note && <div className="truncate text-xs text-qb-ink-muted">{d.note}</div>}
                    </td>
                    <td className="qb-grid-cell text-qb-ink-muted">{[d.nameHint, d.documentDate && formatDate(d.documentDate)].filter(Boolean).join(' · ')}</td>
                    <td className="qb-grid-cell num">{d.amountCents ? formatCents(d.amountCents) : ''}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
        <section className="qb-panel flex min-h-0 flex-col" aria-label="Preview">
          {selected ? (
            <>
              <div className="flex flex-none flex-wrap items-center gap-2 border-b border-qb-border px-2 py-1">
                <span className="min-w-0 flex-1 truncate font-semibold" title={selected.fileName}>
                  {selected.fileName}
                </span>
                <button type="button" className="qb-btn qb-btn-primary h-row" onClick={() => enter('expenses/bills', 'bill')}>
                  Enter as bill
                </button>
                <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => enter('expenses/expenses', 'expense')}>
                  Expense
                </button>
                <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => enter('expenses/expenses', 'check')}>
                  Check
                </button>
                <button
                  type="button"
                  className="qb-btn qb-btn-secondary h-row text-qb-negative"
                  onClick={() => act(() => api.removeInboxDocument(selected.id), `Removed ${selected.fileName} from the inbox.`)}
                >
                  Remove
                </button>
              </div>
              <div className="flex flex-none items-center gap-2 border-b border-qb-border px-2 py-1 text-sm">
                <label className="flex min-w-0 flex-1 items-center gap-1">
                  <span className="text-qb-ink-muted">Note</span>
                  <input className="qb-input min-w-0 flex-1" value={note} onChange={(e) => setNote(e.target.value)} placeholder="Who to ask, which job…" maxLength={500} />
                </label>
                <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={note === (selected.note ?? '')} onClick={() => act(() => api.setInboxDocumentNote(selected.id, note), null)}>
                  Save note
                </button>
              </div>
              <div className="min-h-0 flex-1 overflow-auto bg-qb-canvas">
                <Preview doc={selected} />
              </div>
            </>
          ) : (
            <p className="p-3 text-sm text-qb-ink-muted">Select a document to see it.</p>
          )}
        </section>
      </div>
    </div>
  );
}
