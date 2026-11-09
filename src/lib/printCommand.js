import { useEffect, useRef } from 'react';
import { printDocument } from './api.js';

/** Screens that print something of their own for File › Print (a register, say), newest last. */
const printers = [];

/**
 * File › Print… (Ctrl+P) on this screen runs `print` while the element `ref` points at is showing,
 * instead of printing the window as it is.
 */
export function usePrintCommand(ref, print) {
  const latest = useRef(print);
  latest.current = print;
  useEffect(() => {
    const entry = { root: () => ref.current, print: () => latest.current() };
    printers.push(entry);
    return () => {
      const at = printers.indexOf(entry);
      if (at >= 0) printers.splice(at, 1);
    };
  }, [ref]);
}

/**
 * File › Print…: the showing screen's own print (the one holding focus, else the newest showing),
 * or else the window as it is.
 */
export function runPrintCommand() {
  const showing = printers.filter((p) => {
    const root = p.root();
    return root?.isConnected && root.getClientRects().length > 0;
  });
  const target = showing.find((p) => p.root().contains(document.activeElement)) ?? showing[showing.length - 1];
  if (target) return target.print();
  return printDocument();
}
