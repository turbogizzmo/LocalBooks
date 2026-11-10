// PDF pages as pictures, drawn here rather than in a frame: the app's content security policy
// (tauri.conf.json) lets no frame show a file, so a PDF is rendered page by page with pdf.js into a
// canvas. pdf.js runs in a worker served from the app; text is drawn as outlines rather than loaded
// as web fonts, and nothing is evaluated as code (isEvalSupported off), so the policy stays as it is.

let loaded = null;

/** pdf.js, loaded on first use (it is large) with its worker from the app bundle. */
async function pdfjs() {
  if (!loaded) {
    loaded = Promise.all([import('pdfjs-dist/legacy/build/pdf.min.mjs'), import('pdfjs-dist/legacy/build/pdf.worker.min.mjs?url')]).then(([lib, worker]) => {
      lib.GlobalWorkerOptions.workerSrc = worker.default;
      return lib;
    });
  }
  return loaded;
}

/** The most pages drawn for a preview or a printout, so a huge file can't stall the window. */
export const MAX_PAGES = 50;

/**
 * Draws each page of the PDF in `bytes` (a Uint8Array or ArrayBuffer) and returns them as PNG data
 * URLs with their size in CSS pixels, at `scale` (1 = 72 dpi). `onPage` sees each one as it is done.
 */
export async function renderPdf(bytes, { scale = 1.5, maxPages = MAX_PAGES, onPage } = {}) {
  const lib = await pdfjs();
  const data = bytes instanceof Uint8Array ? bytes.slice() : new Uint8Array(bytes);
  const task = lib.getDocument({
    data,
    isEvalSupported: false,
    disableFontFace: true,
    standardFontDataUrl: `${import.meta.env.BASE_URL}pdfjs/standard_fonts/`,
    stopAtErrors: false,
  });
  const doc = await task.promise;
  try {
    const pages = [];
    const count = Math.min(doc.numPages, maxPages);
    for (let n = 1; n <= count; n += 1) {
      const page = await doc.getPage(n);
      const viewport = page.getViewport({ scale });
      const canvas = document.createElement('canvas');
      canvas.width = Math.ceil(viewport.width);
      canvas.height = Math.ceil(viewport.height);
      const canvasContext = canvas.getContext('2d');
      if (!canvasContext) throw new Error('there is no memory left to draw the page; close other windows and try again');
      // The print intent draws in one go rather than frame by frame, so pages still render while the
      // window is in the background (and, as pictures, are what a printout needs anyway).
      await page.render({ canvas, canvasContext, viewport, intent: 'print' }).promise;
      const shown = { url: canvas.toDataURL('image/png'), width: viewport.width / scale, height: viewport.height / scale };
      pages.push(shown);
      onPage?.(shown, n, doc.numPages);
      page.cleanup();
    }
    return { pages, pageCount: doc.numPages };
  } finally {
    await task.destroy();
  }
}

/** Base64 (as the backend sends file contents) to bytes. */
export function bytesOf(base64) {
  const text = atob(base64);
  const out = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i += 1) out[i] = text.charCodeAt(i);
  return out;
}
