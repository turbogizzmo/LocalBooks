// Company Snapshot helpers (see components/CompanySnapshot.jsx).

/** The widget ids with the one at `index` moved `by` places (−1 earlier, +1 later), kept in range. */
export function moveWidget(ids, index, by) {
  const to = Math.min(Math.max(index + by, 0), ids.length - 1);
  const next = [...ids];
  const [moved] = next.splice(index, 1);
  next.splice(to, 0, moved);
  return next;
}
