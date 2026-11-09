// Comments on report figures, and the reports kept with them.
import { invoke } from '@tauri-apps/api/core';

export default {
  commentOnReportFigure: (input) => invoke('comment_on_report_figure', { input }),
  getCommentedReport: (id) => invoke('get_commented_report', { id }),
  listCommentedReports: () => invoke('list_commented_reports'),
  deleteReportComment: (id) => invoke('delete_report_comment', { id }),
  deleteCommentedReport: (id) => invoke('delete_commented_report', { id }),
};
