// Edit-menu commands for the open list (gap T33): New, Edit, Delete, Make Inactive (or Active) and
// Show All, sent from the menu bar as `list:<command>` and run by the list on screen. A list
// registers with `useListCommands(ref, handlers)`; the menu reaches the list that has focus, or else
// the visible one registered last, as the form commands do (formFlow.jsx).
import { useEffect, useRef } from 'react';

export const LIST_COMMANDS = ['new', 'edit', 'delete', 'toggle-active', 'show-all'];

const openLists = [];

/** `handlers` maps a command to a function; `ref` is the list's root element. */
export function useListCommands(ref, handlers) {
  const latest = useRef(handlers);
  latest.current = handlers;
  useEffect(() => {
    const entry = { root: () => ref.current, has: (name) => typeof latest.current[name] === 'function', run: (name) => latest.current[name]() };
    openLists.push(entry);
    return () => {
      const at = openLists.indexOf(entry);
      if (at >= 0) openLists.splice(at, 1);
    };
  }, [ref]);
}

/** Picks the list a command goes to: the one holding focus, else the last visible one. */
export function pickList(lists, active) {
  const visible = lists.map((l) => ({ l, root: l.root() })).filter(({ root }) => root?.isConnected && root.getClientRects().length > 0);
  return (visible.find(({ root }) => root.contains(active)) ?? visible[visible.length - 1])?.l ?? null;
}

/** Runs a list command; false when no list is on screen or the list has no such command. */
export function runListCommand(name) {
  if (!LIST_COMMANDS.includes(name)) return false;
  const target = pickList(openLists, typeof document === 'undefined' ? null : document.activeElement);
  if (!target?.has(name)) return false;
  target.run(name);
  return true;
}
