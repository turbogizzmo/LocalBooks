const STYLES = {
  error: 'border-qb-negative/40 bg-[#fdf0ef] text-qb-negative',
  success: 'border-qb-green/40 bg-qb-green-tint text-qb-green-active',
  info: 'border-qb-info/40 bg-[#eaf4fb] text-qb-info',
};

export default function Notice({ kind = 'info', text, onDismiss }) {
  if (!text) return null;
  return (
    <div
      role={kind === 'error' ? 'alert' : 'status'}
      className={`flex items-center gap-2 border px-2 py-0.5 text-sm ${STYLES[kind]}`}
    >
      <span className="min-w-0 flex-1 truncate" title={text}>
        {text}
      </span>
      {onDismiss && (
        <button type="button" onClick={onDismiss} className="px-1 font-semibold opacity-70 hover:opacity-100" aria-label="Dismiss">
          ×
        </button>
      )}
    </div>
  );
}
