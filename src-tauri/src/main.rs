#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup;
mod bank;
mod banking;
mod commands;
mod db;
mod estimates;
mod expenses;
mod export_engine;
mod invoices;
mod ledger;
mod preferences;
mod qbw_import;
mod reconcile;
mod reports;
mod sales;
mod statements;
mod storage;
mod timesheet;

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let config = db::DbConfig {
                config_dir: app.path().app_config_dir()?,
                default_path: data_dir.join(storage::DB_FILE_NAME),
                backups_dir: data_dir.join("backups"),
            };
            // Honours config.json; falls back to the local file (with a warning) if that location is unavailable.
            let database = tauri::async_runtime::block_on(db::Db::startup(config))?;
            app.manage(database);

            // Daily automatic backup, off the startup path.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let db = handle.state::<db::Db>();
                if let Err(err) = backup::auto_backup(db.inner()).await {
                    eprintln!("automatic backup failed: {err}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::get_dashboard,
            commands::post_journal_entry,
            commands::list_invoices,
            commands::create_invoice,
            commands::record_invoice_payment,
            commands::list_invoice_payments,
            commands::get_ar_aging,
            commands::parse_bank_csv,
            commands::import_bank_csv,
            commands::list_bank_transactions,
            commands::approve_bank_transaction,
            commands::exclude_bank_transaction,
            commands::get_reconcile_state,
            commands::get_unreconciled_lines,
            commands::list_reconciliations,
            commands::start_reconciliation,
            commands::set_lines_cleared,
            commands::lock_reconciliation,
            commands::cancel_reconciliation,
            commands::get_profit_and_loss,
            commands::get_balance_sheet,
            commands::list_customers,
            commands::create_customer,
            commands::list_products,
            commands::create_product,
            commands::post_sales_receipt,
            commands::list_all_sales,
            commands::list_vendors,
            commands::create_vendor,
            commands::list_bills,
            commands::post_vendor_bill,
            commands::pay_vendor_bill,
            commands::pay_vendor_bills,
            commands::list_bill_payments,
            commands::post_expense,
            commands::list_expenses,
            commands::record_invoice_payments,
            commands::void_transaction,
            commands::list_journal_entries,
            commands::get_journal_entry_lines,
            commands::list_audit_log,
            commands::list_undeposited,
            commands::post_bank_deposit,
            commands::post_transfer,
            commands::list_banking_activity,
            commands::list_estimates,
            commands::create_estimate,
            commands::set_estimate_status,
            commands::convert_estimate_to_invoice,
            commands::list_employees,
            commands::create_employee,
            commands::create_time_activities,
            commands::list_time_activities,
            commands::export_company_file,
            commands::inspect_company_file,
            commands::import_company_file,
            commands::list_backups,
            commands::backup_now,
            commands::get_preferences,
            commands::set_preference,
            commands::list_tax_forms,
            commands::get_fiscal_periods,
            commands::get_tax_calendar,
            commands::report_csv,
            commands::save_report_to_disk,
            commands::print_report,
            commands::get_storage_status,
            commands::inspect_database_directory,
            commands::update_database_directory,
            commands::set_company_logo,
            commands::clear_company_logo,
            commands::get_company_logo,
            commands::get_invoice_detail,
            commands::ingest_raw_qbw,
            commands::list_qbw_batches,
            commands::get_qbw_batch,
            commands::set_qbw_mapping,
            commands::auto_create_qbw_accounts,
            commands::post_qbw_batch,
            commands::discard_qbw_batch,
        ])
        .build(tauri::generate_context!())
        .expect("error while building LocalBooks")
        .run(|app, event| {
            // Close the database cleanly (checkpoint, release any shared-location lock) on quit.
            if let tauri::RunEvent::Exit = event {
                tauri::async_runtime::block_on(app.state::<db::Db>().shutdown());
            }
        });
}
