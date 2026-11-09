/**
 * What an export says once it is saved: where, and that it is a plain copy (gap R4). The company
 * file can be password-protected; a CSV, Excel, PDF or zip saved from it is not, so it is the
 * user's to keep safe or delete.
 */
export const PLAIN_COPY_NOTE = "It's a plain copy that the company file's password doesn't protect: keep it somewhere safe, or delete it when you're done.";

export function savedCopyText(path, what = null) {
  return `Saved ${what ? `${what} to ` : ''}${path}. ${PLAIN_COPY_NOTE}`;
}
