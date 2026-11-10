// Which open list an Edit-menu list command goes to (gap T33). Kept free of React so the JS unit
// tests, which run without installed packages, can load it.

/** Picks the list a command goes to: the one holding focus, else the last visible one. */
export function pickList(lists, active) {
  const visible = lists.map((l) => ({ l, root: l.root() })).filter(({ root }) => root?.isConnected && root.getClientRects().length > 0);
  return (visible.find(({ root }) => root.contains(active)) ?? visible[visible.length - 1])?.l ?? null;
}
