import { useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';

const STATUS = {
  PASS: { icon: '✓', label: 'OK', className: 'text-qb-green-active' },
  WARN: { icon: '⚠', label: 'Attention', className: 'text-[#a35200]' },
  FAIL: { icon: '✕', label: 'Problem', className: 'text-qb-negative' },
};

const VERDICT = {
  PASS: 'Clean bill of health: every check passed.',
  WARN: 'Healthy, with something Rebuild can tidy up.',
  FAIL: 'Problems found. Each one says what to do next.',
};

function formatBytes(n) {
  if (n >= 1_048_576) return `${(n / 1_048_576).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

function CheckRow({ check }) {
  const [open, setOpen] = useState(check.status !== 'PASS');
  const style = STATUS[check.status];
  return (
    <li className="border-b border-qb-border px-2 py-0.5 text-sm last:border-b-0">
      <div className="flex items-start gap-2">
        <span className={`w-4 flex-none text-center font-bold ${style.className}`} aria-label={style.label}>
          {style.icon}
        </span>
        <span className="w-[170px] flex-none font-semibold">{check.title}</span>
        <span className="min-w-0 flex-1">
          {check.summary}
          {check.fixedByRebuild && <span className="ml-1 text-xs font-semibold text-qb-info">Rebuild fixes this.</span>}
        </span>
        {check.details.length > 0 && (
          <button type="button" className="flex-none text-xs text-qb-info hover:underline" onClick={() => setOpen(!open)}>
            {open ? 'Hide details' : `Details (${check.details.length})`}
          </button>
        )}
      </div>
      {open && check.details.length > 0 && (
        <ul className="mb-0.5 ml-[186px] list-disc pl-4 font-mono text-xs text-qb-ink-muted">
          {check.details.map((d) => (
            <li key={d}>{d}</li>
          ))}
        </ul>
      )}
    </li>
  );
}

function RebuildConfirm({ busy, onConfirm, onCancel }) {
  return (
    <div className="border-t border-qb-border bg-[#fff8eb] px-2 py-1 text-sm">
      <div className="font-semibold">Rebuild the company file?</div>
      <div>
        LocalBooks saves a verified snapshot first, then restores any missing or changed safeguards and indexes from this version, and
        compacts the file. Your transactions are never rewritten. Other screens may pause for a moment while it runs.
      </div>
      <div className="mt-1 flex gap-2">
        <button type="button" className="qb-btn qb-btn-primary" onClick={onConfirm} disabled={busy}>
          {busy ? 'Rebuilding…' : 'Back up and rebuild'}
        </button>
        <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
      </div>
    </div>
  );
}

/** File Dentist: a read-only check-up of the company file, and a backed-up rebuild. */
export default function FileDentist({ onRebuilt }) {
  const [report, setReport] = useState(null);
  const [rebuild, setRebuild] = useState(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(null);
  const [notice, setNotice] = useState(null);

  const runCheckup = async () => {
    setBusy('checkup');
    setNotice(null);
    setRebuild(null);
    try {
      setReport(await api.fileDentistCheckup());
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(null);
    }
  };

  const runRebuild = async () => {
    setBusy('rebuild');
    setNotice(null);
    try {
      const result = await api.fileDentistRebuild();
      setRebuild(result);
      setReport(result.report);
      setConfirming(false);
      onRebuilt?.();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className="qb-panel">
      <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
        File Dentist · Verify &amp; Rebuild
      </h2>
      <div className="flex flex-wrap items-center gap-2 px-2 py-1.5">
        <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" onClick={runCheckup} disabled={busy !== null}>
          {busy === 'checkup' ? 'Checking…' : 'Run Check-up'}
        </button>
        <button
          type="button"
          className="qb-btn qb-btn-secondary h-row-lg px-3"
          onClick={() => setConfirming(true)}
          disabled={busy !== null || !report?.canRebuild}
          title={report ? undefined : 'Run a check-up first'}
        >
          Rebuild…
        </button>
        <span className="min-w-0 flex-1 text-xs text-qb-ink-muted">
          The check-up only reads: file structure, schema, the safeguards that enforce double-entry, record links, balances, posting
          history and stored totals. Rebuild repairs what can be repaired safely, after saving a snapshot.
        </span>
      </div>
      {confirming && <RebuildConfirm busy={busy === 'rebuild'} onConfirm={runRebuild} onCancel={() => setConfirming(false)} />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {rebuild && (
        <div className="border-t border-qb-border bg-qb-green-tint px-2 py-1 text-sm">
          <div className="font-semibold text-qb-green-active">
            Rebuilt. {formatBytes(rebuild.sizeBefore)} → {formatBytes(rebuild.sizeAfter)}. Snapshot saved as {rebuild.safetyBackup.fileName}.
          </div>
          <ul className="list-disc pl-5 text-xs">
            {rebuild.actions.map((a) => (
              <li key={a}>{a}</li>
            ))}
          </ul>
        </div>
      )}
      {report && (
        <div className="border-t border-qb-border">
          <div className={`flex items-center gap-2 px-2 py-0.5 text-sm font-semibold ${STATUS[report.status].className}`}>
            <span>{VERDICT[report.status]}</span>
            <span className="ml-auto text-xs font-normal text-qb-ink-muted">
              {formatBytes(report.sizeBytes)} · checked {report.checkedAt.replace('T', ' ')}
            </span>
          </div>
          <ul className="border-t border-qb-border">
            {report.checks.map((c) => (
              <CheckRow key={c.id} check={c} />
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}
