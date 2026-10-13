#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup;
mod bank;
mod banking;
mod closing;
mod commands;
mod db;
mod estimates;
mod expenses;
mod export_engine;
mod instance;
mod invoices;
mod ledger;
mod mcp_server;
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
            // A double-clicked .localbooks file arrives as a launch argument on Windows and Linux.
            // If LocalBooks is already running, hand the file (or a plain launch) to it and quit.
            let launch_file = storage::company_file_from_args(std::env::args_os());
            let config_dir = app.path().app_config_dir()?;
            if instance::hand_off(&config_dir, &instance::request_for(launch_file.as_deref())) {
                std::process::exit(0);
            }
            // Otherwise the file opens in place; without one, config.json is honoured, falling back
            // to the local file (with a warning) if that location is unavailable.
            let database = tauri::async_runtime::block_on(db::Db::startup_with_file(config, launch_file))?;
            app.manage(database);
            // The local MCP server (off unless turned on in Settings) reads whichever company file is
            // live at the time of each request.
            let pool_handle = app.handle().clone();
            let pools: mcp_server::PoolSource = std::sync::Arc::new(move || {
                let handle = pool_handle.clone();
                Box::pin(async move { handle.state::<db::Db>().pool().await })
            });
            app.manage(mcp_server::McpService::new(app.path().app_config_dir()?, pools));
            let mcp_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { mcp_handle.state::<mcp_server::McpService>().start_if_enabled().await });

            // Accept hand-offs from later launches.
            let handoff_handle = app.handle().clone();
            let guard = tauri::async_runtime::block_on(instance::listen(&config_dir, move |request| {
                let app = handoff_handle.clone();
                tauri::async_runtime::spawn(async move { commands::handle_hand_off(&app, request).await });
            }));
            match guard {
                Ok(guard) => {
                    app.manage(std::sync::Mutex::new(Some(guard)));
                }
                Err(err) => eprintln!("single-instance hand-off unavailable: {err}"),
            }

            let title_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { commands::refresh_window_title(&title_handle).await });

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
            commands::verify_ledger_integrity,
            commands::open_company_file,
            commands::get_closing_status,
            commands::set_closing_date,
            commands::get_mcp_status,
            commands::set_mcp_enabled,
            commands::regenerate_mcp_token,
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
        .run(|app, event| match event {
            // macOS hands double-clicked .localbooks files to the running app as an Opened event,
            // including the file that launched it.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Opened { urls } => {
                let file = urls.iter().filter_map(|url| url.to_file_path().ok()).find(|path| storage::is_company_file(path));
                if let Some(path) = file {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move { commands::handle_hand_off(&app, instance::HandOff::Open { path }).await });
                }
            }
            // Close the database cleanly (checkpoint, release any shared-location lock) on quit.
            tauri::RunEvent::Exit => tauri::async_runtime::block_on(async {
                if let Some(state) = app.try_state::<std::sync::Mutex<Option<instance::InstanceGuard>>>() {
                    drop(state.lock().ok().and_then(|mut guard| guard.take()));
                }
                app.state::<mcp_server::McpService>().shutdown().await;
                app.state::<db::Db>().shutdown().await;
            }),
            _ => {}
        });
}
