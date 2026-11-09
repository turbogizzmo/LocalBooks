// Printed envelopes: the layout in Custom Forms, and an envelope for a customer, vendor or form.
import { invoke } from '@tauri-apps/api/core';

export default {
  getEnvelopeLayout: () => invoke('get_envelope_layout'),
  saveEnvelopeLayout: (layout) => invoke('save_envelope_layout', { layout }),
  resetEnvelopeLayout: () => invoke('reset_envelope_layout'),
  // target: { kind: 'customer', id } | { kind: 'vendor', id } | { kind: 'form', form: 'INVOICE', id }; null = the sample.
  // layout: an unsaved layout to preview with; null = the saved one.
  renderEnvelope: (target = null, layout = null) => invoke('render_envelope', { target, layout }),
};
