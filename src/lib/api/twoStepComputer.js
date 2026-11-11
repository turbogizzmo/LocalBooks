// Two-step sign-in on this computer (gap R29): Touch ID or Windows Hello in place of the code.
import { invoke } from '@tauri-apps/api/core';

export default {
  getTwoStepComputerSignIn: () => invoke('get_two_step_computer_sign_in'),
  finishTwoStepSignInWithComputer: () => invoke('finish_two_step_sign_in_with_computer'),
  getMyTwoStepComputers: () => invoke('get_my_two_step_computers'),
  addThisComputerTwoStep: (password) => invoke('add_this_computer_two_step', { password }),
  removeTwoStepComputer: (id) => invoke('remove_two_step_computer', { id }),
};
