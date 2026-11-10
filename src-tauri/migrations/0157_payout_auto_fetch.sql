-- T68: payout feeds fetched on a schedule while LocalBooks is open (`payout_feeds::fetch_due`).
--
-- Off for every feed until an Admin picks how often (`auto_fetch_hours`: every 1, 6, 12 or 24
-- hours). `last_auto_result` keeps what the last automatic fetch found, or why it failed, for the
-- Payout Feeds screen; it never holds a key.

ALTER TABLE payout_feeds ADD COLUMN auto_fetch_hours INTEGER CHECK (auto_fetch_hours IS NULL OR auto_fetch_hours IN (1, 6, 12, 24));
ALTER TABLE payout_feeds ADD COLUMN last_auto_result TEXT CHECK (last_auto_result IS NULL OR length(last_auto_result) <= 2000);
-- When the last automatic try was, so a failing one waits for the next interval too.
ALTER TABLE payout_feeds ADD COLUMN last_auto_at TEXT;
