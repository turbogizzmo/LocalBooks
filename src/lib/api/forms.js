// Company logo, form designer and PDF forms.
import { invoke } from '@tauri-apps/api/core';

export default {
  listFormStyles: () => invoke('list_form_styles'),
  createFormStyle: (name, copyFrom = null) => invoke('create_form_style', { name, copyFrom }),
  renameFormStyle: (id, name) => invoke('rename_form_style', { id, name }),
  setDefaultFormStyle: (id) => invoke('set_default_form_style', { id }),
  deleteFormStyle: (id) => invoke('delete_form_style', { id }),
  // styleId null = the default style. For STATEMENT forms the document id is the customer's and
  // period is { startDate, endDate } (last month when omitted).
  getFormTemplate: (kind, styleId = null) => invoke('get_form_template', { kind, styleId }),
  saveFormTemplate: (kind, template, styleId = null) => invoke('save_form_template', { kind, template, styleId }),
  resetFormTemplate: (kind, styleId = null) => invoke('reset_form_template', { kind, styleId }),
  previewForm: (kind, template, documentId = null, period = null) => invoke('preview_form', { kind, template, documentId, period }),
  renderForm: (kind, documentId, period = null) => invoke('render_form', { kind, documentId, period }),
  saveFormPdf: (kind, documentId, period = null) => invoke('save_form_pdf', { kind, documentId, period }),
  setCompanyLogo: (sourcePath) => invoke('set_company_logo', { sourcePath }),
  clearCompanyLogo: () => invoke('clear_company_logo'),
  getCompanyLogo: () => invoke('get_company_logo'),
};
