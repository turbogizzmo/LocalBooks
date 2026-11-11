// The backend refuses a form's customer or vendor name that is not on its list, with
// `NAME_NOT_ON_LIST: <name> isn't on your <customer|vendor> list. …` (sales.rs `require_listed`).

const PREFIX = 'NAME_NOT_ON_LIST: ';
const SHAPE = /^NAME_NOT_ON_LIST: ([\s\S]+) isn't on your (customer|vendor) list\./;

/** The kind and name in a `NAME_NOT_ON_LIST` refusal, or null for any other error. */
export function notOnList(err) {
  const text = typeof err === 'string' ? err : err?.message;
  if (typeof text !== 'string' || !text.startsWith(PREFIX)) return null;
  const match = SHAPE.exec(text);
  return match ? { kind: match[2], name: match[1] } : null;
}
