/**
 * The Tillhouse mark ("Keystone"): a capital T under a roof line on a spruce tile. The same drawing
 * as src-tauri/icons/icon.svg, from which the app icons are made. Decorative: the name beside it
 * says what it is.
 */
export default function BrandMark({ size = 20, className = '' }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" className={`flex-none ${className}`} aria-hidden="true" focusable="false">
      <rect width="64" height="64" rx="14" fill="#0b5d57" />
      <path d="M14 24 32 11.5 50 24" fill="none" stroke="#e8b44a" strokeWidth="5" strokeLinecap="round" strokeLinejoin="round" />
      <rect x="15" y="29" width="34" height="7.5" rx="2" fill="#fff" />
      <rect x="28" y="29" width="8" height="24" rx="2" fill="#fff" />
    </svg>
  );
}
