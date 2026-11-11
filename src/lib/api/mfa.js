// Two-step sign-in (gap R26): the code step, setting up an authenticator app, the company setting.
import { invoke } from '@tauri-apps/api/core';

export default {
  getTwoStepSignIn: () => invoke('get_two_step_sign_in'),
  finishTwoStepSignIn: (code) => invoke('finish_two_step_sign_in', { code }),
  cancelTwoStepSignIn: () => invoke('cancel_two_step_sign_in'),
  beginTwoStepSetup: (password) => invoke('begin_two_step_setup', { password }),
  finishTwoStepSetup: (code) => invoke('finish_two_step_setup', { code }),
  getMyTwoStep: () => invoke('get_my_two_step'),
  removeMyTwoStep: (password) => invoke('remove_my_two_step', { password }),
  getTwoStepPolicy: () => invoke('get_two_step_policy'),
  setTwoStepPolicy: (policy) => invoke('set_two_step_policy', { policy }),
  resetTwoStep: (id) => invoke('reset_two_step', { id }),
};
