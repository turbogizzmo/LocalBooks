// Payout feeds from payment processors (Stripe, PayPal).
import { invoke } from '@tauri-apps/api/core';

export default {
  listPayoutFeeds: () => invoke('list_payout_feeds'),
  connectStripeFeed: (key, input) => invoke('connect_stripe_feed', { key, input }),
  connectPaypalFeed: (credentials, input) => invoke('connect_paypal_feed', { credentials, input }),
  updatePayoutFeed: (id, input) => invoke('update_payout_feed', { id, input }),
  disconnectPayoutFeed: (id) => invoke('disconnect_payout_feed', { id }),
  fetchPayouts: (id) => invoke('fetch_payouts', { id }),
  listPayouts: (feedId) => invoke('list_payouts', { feedId }),
  recordPayout: (id) => invoke('record_payout', { id }),
  setPayoutIgnored: (id, ignored) => invoke('set_payout_ignored', { id, ignored }),
};
