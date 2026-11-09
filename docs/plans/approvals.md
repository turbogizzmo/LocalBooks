# Approvals for bills and purchase orders (gaps row T45)

A company can require that bills and purchase orders over an amount, or from a given vendor, are
approved before they count. This note sets out what "not counted" means in a double-entry ledger
where posted entries are never edited, and how approval fits around that.

## The rule that decides the design

An unapproved bill must not be posted. A posted bill is a journal entry (Dr expense, Cr Accounts
Payable); posted entries are never edited or deleted (AGENTS.md rule 7), so posting first and
"un-posting" on rejection would leave a void pair in the books for every rejection. Purchase
orders post nothing, but an unapproved one must stay out of Open Purchase Orders, receiving and
billing.

So an unapproved document is **not a bill or purchase order yet**. It is an **approval request**
that holds what was entered. Approving creates the real bill (posted through the ledger, like any
bill) or purchase order, in the same write transaction that marks the request approved.
Rejecting keeps the request, with the approver's note, and creates nothing.

This keeps every existing rule intact: the bills table, its status checks and triggers, A/P,
aging, Pay Bills and Open POs see only approved documents, without a new status to filter out
everywhere.

## Pieces

- **Rules** (`approval_rules`): a kind (bill or purchase order), a minimum total (0 means every
  one), and optionally one vendor. A document needs approval when any rule of its kind matches.
  Only an Admin can add or remove rules.
- **Requests** (`approval_requests`): kind, vendor name, total, document date, the entered form as
  JSON (the same input the bill or PO command takes), status `PENDING`, `APPROVED` or `REJECTED`,
  who asked and when, who decided and when, the note, and the bill or PO created on approval.
  Requests are kept for good; rejected ones are the record of what was turned down and why.
- **Where it is enforced:** the `post_vendor_bill` and `create_purchase_order` commands (what the
  Enter Bills and Create Purchase Orders forms call) refuse with `APPROVAL_REQUIRED` when a rule
  matches. The form then submits the same input as a request and says it was sent for approval.
  Other ways a bill appears are not held: imports and conversions (they record history), bills
  made from an approved purchase order, and memorized bills a user enters again.
- **Who approves:** `approve_request` and `reject_request` are Admin commands in `permissions.rs`.
  In a company file without users everyone is Admin, so approval is a second look rather than a
  control; the guide says so. A rejection needs a note.
- **Approving** re-validates the stored input (the chart or closing date may have changed since it
  was asked for); if posting fails the request stays pending and the error is shown. Approving
  twice is refused (`WHERE status = 'PENDING'`).
- **Condense** keeps both tables whole (they are settings and a history, not ledger rows). The
  bill or PO id on an approved request is a plain id, not a foreign key, so a condensed bill does
  not block condensing.

## Screens

Expenses › Approvals: requests waiting, approved and rejected, each with its vendor, total, date
and who asked; Approve, and Reject with a note, for Admins; and the rules, which Admins edit.
Enter Bills and Create Purchase Orders say "Sent for approval" instead of "posted" when a rule
holds the document.

## Not in this change

Editing a pending request (the requester enters it again and the approver rejects the old one),
holding an edit to a bill that was already approved,
approval chains with more than one approver, and email notifications.
