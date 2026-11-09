// A PDF shown as its pages (see src/lib/pdfPages.js for why it isn't framed).
import { useEffect, useState } from 'react';
import { renderPdf } from '../lib/pdfPages.js';

/** `bytes`: the PDF. Pages appear as they are drawn; a file that isn't a readable PDF says so. */
export default function PdfPreview({ bytes, name = 'PDF', className = 'max-h-[320px]' }) {
  const [pages, setPages] = useState([]);
  const [state, setState] = useState({ busy: true, error: null, total: 0 });
  useEffect(() => {
    let live = true;
    setPages([]);
    setState({ busy: true, error: null, total: 0 });
    renderPdf(bytes, { onPage: (p) => live && setPages((ps) => [...ps, p]) }).then(
      (r) => live && setState({ busy: false, error: null, total: r.pageCount }),
      (err) => live && setState({ busy: false, error: err?.message ?? String(err), total: 0 }),
    );
    return () => {
      live = false;
    };
  }, [bytes]);
  if (state.error) return <span className="px-2 text-center text-qb-ink-muted">This PDF can't be shown here ({state.error}). Save a copy to open it.</span>;
  return (
    <div className={`w-full overflow-auto ${className}`} aria-label={`Preview of ${name}`}>
      {pages.map((p, i) => (
        <img key={i} src={p.url} alt={`${name}, page ${i + 1}`} className="mx-auto mb-2 max-w-full border border-qb-border bg-white" />
      ))}
      {state.busy && <p className="text-center text-xs text-qb-ink-muted">Drawing page {pages.length + 1}…</p>}
      {!state.busy && state.total > pages.length && <p className="text-center text-xs text-qb-ink-muted">The first {pages.length} of {state.total} pages are shown. Save a copy to see them all.</p>}
    </div>
  );
}
