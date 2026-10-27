//! Tauri command surface. Every successful write emits `ledger-changed` so open views refresh live.
//!
//! One file per feature. Each file names its own commands with `handlers![...]`, one per line, and
//! the sorted `features!` list below names each file once, so parallel features touch different
//! lines. `main.rs` registers [`handler`] and never changes for a new command.

/// Names a feature file's commands: its `COMMANDS`, by the name the screens invoke, and a
/// `handler()` that runs them.
macro_rules! handlers {
    ($($command:ident),* $(,)?) => {
        /// This feature's commands, by the name the screens invoke.
        pub const COMMANDS: &[&str] = &[$(stringify!($command)),*];

        /// Runs one of [`COMMANDS`]; returns false for any other command, like `generate_handler!`.
        pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($command),*]
        }
    };
}

/// Declares the feature files and combines their commands into [`handler`] and [`all_commands`].
macro_rules! features {
    ($($feature:ident),* $(,)?) => {
        $(mod $feature;)*

        /// Every registered command, feature by feature.
        #[cfg(test)]
        pub fn all_commands() -> impl Iterator<Item = &'static str> {
            [$($feature::COMMANDS),*].into_iter().flatten().copied()
        }

        /// The app's invoke handler: hands each command to the feature that registered it.
        pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
            type Handler = Box<dyn Fn(tauri::ipc::Invoke) -> bool + Send + Sync>;
            let features: Vec<(&'static [&'static str], Handler)> = vec![$(($feature::COMMANDS, Box::new($feature::handler()))),*];
            move |invoke| match features.iter().find(|(commands, _)| commands.contains(&invoke.message.command())) {
                Some((_, handler)) => handler(invoke),
                None => false,
            }
        }
    };
}

mod prelude;

// One line per feature file, alphabetical.
features![
    about,
    accountant_reports,
    accounts,
    ach,
    attachments,
    backups,
    bank_feeds,
    budgets,
    classes,
    cleanup,
    company_files,
    condense,
    credits,
    custom_fields,
    custom_reports,
    customer_statements,
    deposits,
    desktop,
    editing,
    email,
    estimates,
    expenses,
    finance_charges,
    forms,
    graphs,
    installed_quickbooks,
    inventory,
    invoices,
    item_prices,
    job_reports,
    journal,
    lists,
    mcp,
    migration,
    mileage,
    preferences,
    price_levels,
    purchase_orders,
    reconcile,
    recurring,
    reminders,
    repair,
    report_export,
    revaluation,
    sales,
    sales_lists,
    sales_tax,
    search,
    statements,
    timesheet,
    users,
    vendor_1099,
];

pub use company_files::{close_company, company_changed, handle_hand_off, record_startup_failure, StartupFailure};
pub use users::sign_out_after_idle;

#[cfg(test)]
mod tests {
    /// Each command is registered by exactly one feature; a second would never run.
    #[test]
    fn every_command_is_registered_once() {
        let mut seen = std::collections::BTreeSet::new();
        let twice: Vec<_> = super::all_commands().filter(|name| !seen.insert(*name)).collect();
        assert!(twice.is_empty(), "registered twice: {twice:?}");
        assert!(seen.len() > 200, "found the commands");
    }
}
