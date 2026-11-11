//! The company file opening right now, if any (gap R28): the window shows *Opening <name>…* until it ends.

use super::prelude::*;
use crate::opening::{Opening, OpeningState};

handlers![get_opening_state];

/// Which company file is opening and its current step; `None` when none is.
#[tauri::command]
pub fn get_opening_state(opening: State<'_, Opening>) -> Option<OpeningState> {
    opening.current()
}
