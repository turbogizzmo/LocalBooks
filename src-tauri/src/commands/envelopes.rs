//! Printed envelopes (`envelopes.rs`): the layout in Custom Forms, and an envelope for a
//! customer, a vendor or a sales form's Bill To.

use super::prelude::*;
use crate::envelopes::{self, EnvelopeLayout, EnvelopeTarget, RenderedEnvelope};

handlers![
    get_envelope_layout,
    save_envelope_layout,
    reset_envelope_layout,
    render_envelope,
];

#[tauri::command]
pub async fn get_envelope_layout(db: State<'_, Db>) -> Result<EnvelopeLayout, String> {
    envelopes::get_layout(&db.pool().await).await
}

#[tauri::command]
pub async fn save_envelope_layout(db: State<'_, Db>, layout: EnvelopeLayout) -> Result<EnvelopeLayout, String> {
    envelopes::save_layout(&db.pool().await, layout).await
}

#[tauri::command]
pub async fn reset_envelope_layout(db: State<'_, Db>) -> Result<EnvelopeLayout, String> {
    envelopes::reset_layout(&db.pool().await).await
}

/// An envelope laid out for the screen and printing. `target` None is the sample; `layout` an
/// unsaved layout (the Custom Forms preview), else the saved one.
#[tauri::command]
pub async fn render_envelope(db: State<'_, Db>, target: Option<EnvelopeTarget>, layout: Option<EnvelopeLayout>) -> Result<RenderedEnvelope, String> {
    envelopes::render(&db.pool().await, target, layout).await
}
