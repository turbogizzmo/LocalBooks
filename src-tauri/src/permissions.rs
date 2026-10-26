//! What each signed-in role may do, checked in one place before any command runs (see `main.rs`).
//!
//! A company file without users lets everything through, as before users existed. With users on:
//! - [`Need::Open`] commands (signing in, opening or closing a company file, windows) always run;
//! - everything else needs someone signed in, who has replaced any temporary password;
//! - [`Need::Read`] commands run for every role;
//! - [`Need::Write`] commands (anything that changes the books) need Admin or Standard;
//! - [`Need::Admin`] commands (users, company-wide settings, imports and restores, bank connections,
//!   the AI access server) need an Admin.
//!
//! An unlisted command counts as a write, so a new command is closed to View-only users until it is
//! classified here. The rules are LocalBooks' own: the company file is still a SQLite file that
//! another program can read and change; only encrypted values ([`crate::crypto`]) stay protected,
//! and the ledger audit trail shows changes made outside LocalBooks.

use crate::users::Role;

/// Refused because the signed-in user was idle past the company's limit; the caller signs them out.
pub const IDLE_SIGNED_OUT: &str = "SIGN_IN_IDLE: you were signed out after a while without activity; sign in again";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Open,
    Read,
    Write,
    Admin,
}

const OPEN: &[&str] = &[
    "get_startup_failure",
    "get_third_party_notices",
    "get_storage_status",
    "get_sign_in_state",
    "turn_on_users",
    "sign_in",
    "sign_out",
    "sign_in_remembered",
    "forget_remembered_sign_in",
    "sign_out_if_idle",
    "change_my_password",
    "recover_with_recovery_key",
    "remove_all_users",
    "get_start_screen",
    "open_company_file",
    "close_company_file",
    "create_company_file",
    "forget_recent_company",
    "find_company_files",
    "restore_backup_as",
    "open_window",
];

const ADMIN: &[&str] = &[
    // Users and the recovery key.
    "list_users",
    "add_user",
    "update_user",
    "replace_recovery_key",
    // Company-wide settings.
    "set_preference",
    "set_company_logo",
    "clear_company_logo",
    "set_closing_date",
    "create_form_style",
    "rename_form_style",
    "set_default_form_style",
    "delete_form_style",
    "save_form_template",
    "reset_form_template",
    "get_smtp_settings",
    "save_smtp_settings",
    "test_smtp_settings",
    // A bank account's ACH originator details (what the bank's files are sent as).
    "set_ach_originator",
    "send_smtp_test_email",
    "get_mcp_status",
    "set_mcp_enabled",
    "regenerate_mcp_token",
    // Where the file lives, whole-file copies, restores and repairs.
    "inspect_database_directory",
    "update_database_directory",
    "export_company_file",
    "import_company_file",
    "file_dentist_rebuild",
    "apply_forensic_correction",
    // Bank connections (their access tokens).
    "simplefin_connect",
    "simplefin_reconnect",
    "simplefin_refresh_accounts",
    "simplefin_link",
    "simplefin_unlink",
    "simplefin_disconnect",
    // Importing other programs' books.
    "ingest_raw_qbw",
    "ingest_report_exports",
    "set_qbw_mapping",
    "auto_create_qbw_accounts",
    "post_qbw_batch",
    "discard_qbw_batch",
    "reverse_qbw_batch",
    "set_ledger_account",
    "auto_map_ledger_accounts",
    "import_ledger_lists",
    "post_ledger_batch",
    "reverse_ledger_batch",
    "discard_ledger_batch",
    "start_over_ledger_batch",
];

/// Commands that only read, besides those named `get_…`, `list_…`, `inspect_…` and `preview_…`.
const READ: &[&str] = &[
    // Counts as activity (see `main.rs`); refused like any other command once the limit has passed.
    "touch_session",
    "search_books",
    "find_transactions",
    "verify_ledger_integrity",
    "file_dentist_checkup",
    "propose_forensic_corrections",
    "simplefin_suggested_start",
    "report_csv",
    "save_report_to_disk",
    "save_xlsx",
    "save_report_xlsx",
    "print_report",
    "render_form",
    "save_form_pdf",
    "save_attachment",
    "backup_now",
];

pub fn need(command: &str) -> Need {
    if OPEN.contains(&command) {
        Need::Open
    } else if ADMIN.contains(&command) {
        Need::Admin
    } else if READ.contains(&command) || ["get_", "list_", "inspect_", "preview_"].iter().any(|p| command.starts_with(p)) {
        Need::Read
    } else {
        Need::Write
    }
}

/// Who may do what in the open company file, kept current by [`crate::db::Db::refresh_gate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// Not worked out yet: only [`Need::Open`] commands run.
    Unchecked,
    /// No company file open: commands report that themselves.
    NoCompany,
    /// A company file without users.
    NoUsers,
    /// Users are on; who is signed in, if anyone.
    Users(Option<SignedIn>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedIn {
    pub role: Role,
    pub must_change_password: bool,
}

pub fn check(gate: &Gate, need: Need) -> Result<(), String> {
    if need == Need::Open {
        return Ok(());
    }
    let user = match gate {
        Gate::NoCompany | Gate::NoUsers => return Ok(()),
        Gate::Unchecked => return Err("SIGN_IN_REQUIRED: the company file is still opening; try again".into()),
        Gate::Users(None) => return Err(crate::users::SIGN_IN_REQUIRED.into()),
        Gate::Users(Some(user)) => user,
    };
    if user.must_change_password {
        return Err("PASSWORD_CHANGE_REQUIRED: choose your own password first".into());
    }
    match (need, user.role) {
        (Need::Write, Role::ViewOnly) => Err("ROLE_VIEW_ONLY: view only: your user can look at the books but not change them".into()),
        (Need::Admin, Role::Standard | Role::ViewOnly) => Err("ROLE_ADMIN_ONLY: only an Admin can do this".into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
