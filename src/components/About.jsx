import { useEffect, useState } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { api } from '../lib/api.js';
import { Icon } from '../lib/icons.js';

export const INTUIT_DISCLAIMER =
  'QuickBooks® is a registered trademark of Intuit Inc. LocalBooks is an independent project and is not affiliated with, endorsed by, or sponsored by Intuit Inc.';

/** Help › About LocalBooks: version, license, trademark notice and the bundled third-party notices. */
export default function About() {
  const [version, setVersion] = useState('');
  const [notices, setNotices] = useState(null);

  useEffect(() => {
    getVersion().then(setVersion, () => setVersion(''));
  }, []);

  const openNotices = () => {
    setNotices({ text: null });
    api.getThirdPartyNotices().then(
      (text) => setNotices({ text }),
      (err) => setNotices({ error: String(err) }),
    );
  };

  return (
    <section aria-label="About LocalBooks" className="border-t border-qb-border px-3 pb-3 pt-3">
      <h3 className="pb-1 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">About LocalBooks</h3>
      <p className="leading-snug">
        LocalBooks {version} is open source under the MIT License and includes open-source components under their own licenses.
      </p>
      <button type="button" onClick={openNotices} className="mt-1 inline-flex items-center gap-1 font-semibold text-qb-green-active hover:underline">
        Third-party notices
      </button>
      <p className="mt-2 leading-snug text-qb-ink-muted">{INTUIT_DISCLAIMER}</p>
      {notices && <NoticesDialog notices={notices} onClose={() => setNotices(null)} />}
    </section>
  );
}

function NoticesDialog({ notices, onClose }) {
  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
      }
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-[60] grid place-items-center bg-qb-nav/40 backdrop-blur-[2px]">
      <div role="dialog" aria-modal="true" aria-label="Third-party notices" className="flex h-[80vh] w-[760px] max-w-[92vw] flex-col rounded-xs bg-white shadow-popover">
        <div className="flex flex-none items-center border-b border-qb-border px-3 py-1.5">
          <h2 className="text-md font-semibold">Third-party notices</h2>
          <button type="button" onClick={onClose} className="ml-auto rounded-xs p-1 text-qb-ink-muted hover:bg-qb-row-hover hover:text-qb-ink" aria-label="Close">
            <Icon name="Close" />
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto p-3 scrollbar-thin">
          {notices.error ? (
            <p className="text-sm text-qb-negative">The notices could not be read: {notices.error}</p>
          ) : notices.text === null ? (
            <p className="text-sm text-qb-ink-muted">Loading…</p>
          ) : (
            <pre className="whitespace-pre-wrap font-mono text-2xs leading-snug">{notices.text}</pre>
          )}
        </div>
      </div>
    </div>
  );
}
