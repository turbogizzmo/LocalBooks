// Search.
import { invoke } from '@tauri-apps/api/core';

export default {
  searchBooks: (query) => invoke('search_books', { query }),
  findTransactions: (query) => invoke('find_transactions', { query }),
};
