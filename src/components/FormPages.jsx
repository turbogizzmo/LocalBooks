import { useCallback, useEffect, useState } from 'react';
import { api, errorText, printDocument } from '../lib/api.js';
import { PrintPortal } from './PrintDocuments.jsx';

// The standard PDF fonts, as the screen and the print dialog show them.
const FONT_STACKS = {
  HELVETICA: 'Helvetica, Arial, sans-serif',
  TIMES: '"Times New Roman", Times, serif',
  COURIER: '"Courier New", Courier, monospace',
};
const ANCHOR = { left: 'start', right: 'end', center: 'middle' };

/**
 * One page of a laid-out form (see forms.rs), drawn as SVG at the page's size in points. The same
 * operations are written to the PDF, so the preview and the printout match the file.
 */
export function FormSheet({ page, logo, className = '' }) {
  return (
    <svg
      className={`form-sheet ${className}`}
      viewBox={`0 0 ${page.width} ${page.height}`}
      xmlns="http://www.w3.org/2000/svg"
      role="img"
      aria-label="Form page"
    >
      <rect x="0" y="0" width={page.width} height={page.height} fill="#ffffff" />
      {page.ops.map((op, i) => {
        switch (op.op) {
          case 'text':
            return (
              <text
                key={i}
                x={op.x}
                y={op.y}
                fill={op.color}
                fontFamily={FONT_STACKS[op.family]}
                fontSize={op.size}
                fontWeight={op.bold ? 700 : 400}
                textAnchor={ANCHOR[op.align]}
                xmlSpace="preserve"
              >
                {op.text}
              </text>
            );
          case 'rect':
            return <rect key={i} x={op.x} y={op.y} width={op.w} height={op.h} fill={op.fill ?? 'none'} stroke={op.stroke ?? 'none'} strokeWidth={op.lineWidth} />;
          case 'line':
            return <line key={i} x1={op.x1} y1={op.y1} x2={op.x2} y2={op.y2} stroke={op.color} strokeWidth={op.width} />;
          case 'image':
            return logo ? <image key={i} href={logo} x={op.x} y={op.y} width={op.w} height={op.h} preserveAspectRatio="xMidYMid meet" /> : null;
          default:
            return null;
        }
      })}
    </svg>
  );
}

/**
 * Prints and saves invoices and estimates with their designed form. Returns [portal element to
 * render, { print(kind, id), savePdf(kind, id) }]; results are reported through `onNotice`.
 */
export function useFormPrinter(onNotice) {
  const [form, setForm] = useState(null);

  useEffect(() => {
    if (!form) return undefined;
    // Once printed, empty the print root so printing the screen itself works again.
    const clear = () => setForm(null);
    window.addEventListener('afterprint', clear);
    // Let React commit the pages (and the logo image decode) before the dialog snapshots the page.
    const timer = setTimeout(printDocument, 80);
    return () => {
      clearTimeout(timer);
      window.removeEventListener('afterprint', clear);
    };
  }, [form]);

  const print = useCallback(
    async (kind, id) => {
      try {
        setForm(await api.renderForm(kind, id));
      } catch (err) {
        onNotice({ kind: 'error', text: errorText(err) });
      }
    },
    [onNotice],
  );

  const savePdf = useCallback(
    async (kind, id) => {
      try {
        const path = await api.saveFormPdf(kind, id);
        if (path) onNotice({ kind: 'success', text: `Saved ${path}` });
      } catch (err) {
        onNotice({ kind: 'error', text: errorText(err) });
      }
    },
    [onNotice],
  );

  const portal = form ? (
    <PrintPortal>
      {form.pages.map((page, i) => (
        <FormSheet key={i} page={page} logo={form.logo} />
      ))}
    </PrintPortal>
  ) : null;
  return [portal, { print, savePdf }];
}

/** Row-menu entries for a document's form. */
export function formMenuItems(kind, id, actions) {
  return [
    { label: 'Print…', onSelect: () => actions.print(kind, id) },
    { label: 'Save as PDF…', onSelect: () => actions.savePdf(kind, id) },
  ];
}
