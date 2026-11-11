-- T75: a bill made from a purchase order (Create Bill) can be held for approval. The request keeps
-- the order it came from; approving it posts the bill and marks that order billed with the bill's
-- id, in one write transaction (`approvals.rs`). A plain id, like bill_id and purchase_order_id,
-- so condensing or deleting the order isn't blocked by an old request.
ALTER TABLE approval_requests ADD COLUMN from_purchase_order_id INTEGER
    CHECK (from_purchase_order_id IS NULL OR kind = 'BILL');

-- One bill at a time waits for approval per order.
CREATE UNIQUE INDEX idx_approval_requests_pending_po ON approval_requests (from_purchase_order_id)
    WHERE status = 'PENDING' AND from_purchase_order_id IS NOT NULL;
