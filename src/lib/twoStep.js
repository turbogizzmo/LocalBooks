// Two-step sign-in (gap R26): small pure helpers for the screens in components/TwoStep.jsx.

/** Whether a sign-in error means "the password was right; now the code". */
export const needsCode = (err) => String(err?.message ?? err ?? '').startsWith('MFA_CODE_REQUIRED');

/** The QR code the backend drew (an SVG document) as an image source, without inserting markup. */
export const qrImageSource = (svg) => `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;

/** Whether what was typed can be sent: a 6-digit code, or a 10-character backup code. */
export function codeReady(text) {
  const compact = String(text ?? '').replace(/[\s-]/g, '');
  return /^\d{6}$/.test(compact) || /^[0-9A-Za-z]{10}$/.test(compact);
}

/** The company setting's choices: value, label, what it means. */
export const POLICIES = [
  ['OFF', 'Off', 'Nobody is asked for a code. Turning it off removes everyone’s authenticator.'],
  ['OPTIONAL', 'Optional', 'Each user may set up an authenticator app; nobody has to.'],
  ['ADMINS', 'Required for Admins and External Accountants', 'They set one up at their next sign-in; anyone else may.'],
  ['ALL', 'Required for everyone', 'Every user sets one up at their next sign-in.'],
];
