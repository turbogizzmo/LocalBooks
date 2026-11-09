# Undo Last Reconciliation (T54)

## The problem

A finished reconciliation is locked (`reconciliations.is_locked = 1`, migration 0005): its row can't
change or be deleted, its cleared lines carry `reconciled_at` and can't be unticked, and their
entries can't be voided. That is what makes a reconciled balance trustworthy. But people finish a
reconciliation by mistake (the wrong statement, a transaction ticked that wasn't on it) and the
desktop software they come from has **Undo Last Reconciliation** for that.

## The rule

A locked reconciliation is never edited or deleted. Undoing it is a **new, audited record that
reverses the lock**, written once and never changed:

- The locked row stays exactly as it was (statement date, balances, `locked_at`), and so does its
  snapshot (`reconciliation_snapshots`, 0056). What was reconciled stays on record.
- A row in a new append-only table, `reconciliation_undos`, says it was undone: when, how many lines
  it released and their total, and an optional reason. Triggers forbid updating or deleting it.
- Only with that row present may the reconciliation's lines lose their `reconciled_at` and
  `reconciliation_id` (the line trigger `trg_jl_reconcile_rules` is rebuilt with that one
  exception). Reconcile still only changes the clearing columns; it posts nothing.
- The undo is written to both audit trails in the same transaction: `audit_log` gets an
  `UNDO_RECONCILIATION` row (the Audit Log screen), and `ledger_audit_log` gets a sealed, hash-chained
  record (action `EDIT`, payload `kind: "UNDO_RECONCILIATION"`, so the existing table and its
  triggers are not rebuilt while other branches add triggers to it). The Audit Trail report names it.

## What can be undone

Only the **last** reconciliation of an account that hasn't already been undone, and only while no
reconciliation of that account is in progress (an in-progress one began from the balance the last
one left; cancel it first). The insert trigger on `reconciliation_undos` enforces all three, so no
path around the Rust code can undo an earlier one and leave a gap in the chain of statements.
Undoing again steps back one more statement.

## What changes once it is undone

- Its lines are uncleared and can be ticked again (or their transactions voided or edited, as for
  any uncleared transaction).
- "The last reconciled statement" is the one before it: starting a new reconciliation needs a date
  after that one, so the same statement can be reconciled again, and its beginning balance is the
  reconciled balance without the undone lines.
- It is left out of Reconcile's history, the Previous Reconciliation report's choices and the
  Reconciliation Discrepancy report (its lines are expected to be uncleared now). Earlier
  reconciliations' reports are unchanged and still tie out: their own cleared lines are untouched;
  lines the undone one had cleared show as outstanding again, as they now are.
- A service charge or interest earned posted at its start stays posted, like any transaction; it
  is uncleared with the rest and can be voided if it was wrong.

## Not done

- Undo of anything but the last reconciliation.
- Restoring an undone reconciliation (reconcile the statement again instead).
