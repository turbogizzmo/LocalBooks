// Users and signing in.
import { invoke } from '@tauri-apps/api/core';

export default {
  getSignInState: () => invoke('get_sign_in_state'),
  turnOnUsers: (username, displayName, password) => invoke('turn_on_users', { username, displayName: displayName || null, password }),
  signIn: (username, password, remember = false) => invoke('sign_in', { username, password, remember }),
  signInRemembered: () => invoke('sign_in_remembered'),
  forgetRememberedSignIn: () => invoke('forget_remembered_sign_in'),
  touchSession: () => invoke('touch_session'),
  signOutIfIdle: () => invoke('sign_out_if_idle'),
  signOut: () => invoke('sign_out'),
  listUsers: () => invoke('list_users'),
  addUser: (input) => invoke('add_user', { input }),
  updateUser: (id, input) => invoke('update_user', { id, input }),
  changeMyPassword: (current, next) => invoke('change_my_password', { current, next }),
  recoverWithRecoveryKey: (recoveryKey, username, newPassword) => invoke('recover_with_recovery_key', { recoveryKey, username, newPassword }),
  replaceRecoveryKey: () => invoke('replace_recovery_key'),
  removeAllUsers: (confirmation) => invoke('remove_all_users', { confirmation }),
};
