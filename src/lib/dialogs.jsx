import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';

/**
 * In-app replacements for window.confirm and window.prompt. The macOS webview (WKWebView under
 * wry) shows no dialog for those: confirm() returns false and prompt() returns null at once, so
 * every "Are you sure?" silently cancelled. These dialogs look and behave the same on every system.
 */

/** Asks a yes/no question; resolves true only when the person confirms. */
export function askConfirm(message, { confirmLabel = 'OK', danger = false } = {}) {
  return show((done) => <Dialog message={message} confirmLabel={confirmLabel} danger={danger} onDone={done} />, false);
}

/** Asks for a line of text; resolves the text as typed, or null when cancelled. */
export function askText(message, defaultValue = '') {
  return show((done) => <Dialog message={message} input defaultValue={defaultValue} confirmLabel="OK" onDone={done} />, null);
}

function show(render, cancelled) {
  return new Promise((resolve) => {
    const host = document.createElement('div');
    document.body.appendChild(host);
    const root = createRoot(host);
    const done = (value) => {
      root.unmount();
      host.remove();
      resolve(value === undefined ? cancelled : value);
    };
    root.render(render(done));
  });
}

function Dialog({ message, input = false, defaultValue = '', confirmLabel, danger = false, onDone }) {
  const [text, setText] = useState(defaultValue);
  const focusRef = useRef(null);

  useEffect(() => {
    focusRef.current?.focus();
    if (input) focusRef.current?.select();
    const onKey = (e) => {
      if (e.key === 'Escape') onDone(undefined);
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [input, onDone]);

  const submit = (e) => {
    e.preventDefault();
    onDone(input ? text : true);
  };

  return (
    <div className="fixed inset-0 z-[60] grid place-items-center whitespace-normal bg-qb-nav/40 text-left backdrop-blur-[2px]">
      <form role="alertdialog" aria-modal="true" aria-labelledby="ask-message" className="w-[420px] rounded-xs bg-white shadow-popover" onSubmit={submit}>
        <div className="space-y-2 px-3 py-2 text-sm">
          <p id="ask-message" className="whitespace-pre-line">{message}</p>
          {input && <input ref={focusRef} className="qb-input w-full" value={text} onChange={(e) => setText(e.target.value)} />}
        </div>
        <div className="flex justify-end gap-2 border-t border-qb-border px-3 py-1.5">
          <button ref={input ? undefined : focusRef} type="button" className="qb-btn qb-btn-secondary" onClick={() => onDone(undefined)}>
            Cancel
          </button>
          <button type="submit" className={`qb-btn ${danger ? 'bg-qb-negative text-white hover:opacity-90' : 'qb-btn-primary'}`}>
            {confirmLabel}
          </button>
        </div>
      </form>
    </div>
  );
}
