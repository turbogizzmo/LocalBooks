// The To Do list and Reminders.
import { invoke } from '@tauri-apps/api/core';

export default {
  getReminders: () => invoke('get_reminders'),
  getReminderSettings: () => invoke('get_reminder_settings'),
  saveReminderSettings: (settings) => invoke('save_reminder_settings', { settings }),
  listTodos: () => invoke('list_todos'),
  listTodoAssignees: () => invoke('list_todo_assignees'),
  listTodoLinks: (customerId, vendorId) => invoke('list_todo_links', { customerId: customerId ?? null, vendorId: vendorId ?? null }),
  saveTodo: (id, input) => invoke('save_todo', { id: id ?? null, input }),
  setTodoDone: (id, done) => invoke('set_todo_done', { id, done }),
  deleteTodo: (id) => invoke('delete_todo', { id }),
};
