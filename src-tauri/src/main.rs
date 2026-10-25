#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod audit_chain;
mod backup;
mod bank;
mod banking;
mod classes;
mod closing;
mod commands;
mod credits;
mod crypto;
mod db;
mod edits;
mod email_log;
mod estimates;
mod expenses;
mod forensic;
mod forms;
#[cfg(test)]
mod frontend_checks;
mod iif_import;
mod export_engine;
mod file_dentist;
mod file_menu;
mod instance;
mod inventory;
mod invoices;
mod ledger;
mod mail_app;
mod parties;
mod permissions;
mod pdf;
mod mcp_server;
mod preferences;
mod qbw_backup;
mod qbw_company;
mod qbw_documents;
mod qbw_full_import;
mod qbw_import;
mod qbw_ledger_import;
mod qbw_pages;
mod qbw_records;
mod qif_import;
mod reconcile;
mod registers;
mod report_center;
mod reports;
mod revaluation;
mod sales;
mod sales_tax;
mod tax_ids;
#[cfg(test)]
mod test_companies;
mod secrets;
mod simplefin;
mod smtp;
mod statements;
mod storage;
mod text_import;
mod timesheet;
mod users;
mod windows;
mod workflows;

#[cfg(test)]
mod harness;

use tauri::Manager;

/// Where the company file, backups and settings live. Development builds (`tauri dev`, debug)
/// keep their own under a `dev-builds` folder, so a build from an older branch can never open,
/// upgrade or lock the books the installed app uses.
fn data_dirs(app: &tauri::App) -> tauri::Result<(std::path::PathBuf, std::path::PathBuf)> {
    let (data, config) = (app.path().app_data_dir()?, app.path().app_config_dir()?);
    if cfg!(debug_assertions) {
        Ok((data.join("dev-builds"), config.join("dev-builds")))
    } else {
        Ok((data, config))
    }
}

fn main() {
    tauri::Builder::default()
        .menu(|handle| {
            let menu = tauri::menu::Menu::default(handle)?;
            // Tauri's default Help menu is empty on macOS, so the system fills it with a search box
            // that only finds Apple's own help topics. Drop it there (elsewhere it holds About).
            #[cfg(target_os = "macos")]
            if let Some(help) = menu.get(tauri::menu::HELP_SUBMENU_ID) {
                menu.remove(&help)?;
            }
            #[cfg(target_os = "macos")]
            windows::add_new_window_item(handle, &menu)?;
            file_menu::add_company_items(handle, &menu)?;
            Ok(menu)
        })
        .plugin(tauri_plugin_dialog::init())
        // Signed updates from the GitHub releases (see plugins.updater in tauri.conf.json).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .on_menu_event(|app, event| {
            #[cfg(target_os = "macos")]
            windows::on_menu_event(app, event.id().as_ref());
            file_menu::on_menu_event(app, event.id().as_ref());
        })
        .setup(|app| {
            let (data_dir, config_dir) = data_dirs(app)?;
            audit_chain::install_anchor_store(&config_dir);
            let config = db::DbConfig {
                config_dir: config_dir.clone(),
                default_path: data_dir.join(storage::DB_FILE_NAME),
                backups_dir: data_dir.join("backups"),
            };
            // A double-clicked .localbooks file arrives as a launch argument on Windows and Linux.
            // If LocalBooks is already running, hand the file (or a plain launch) to it and quit.
            let launch_file = storage::company_file_from_args(std::env::args_os());
            if instance::hand_off(&config_dir, &instance::request_for(launch_file.as_deref())) {
                std::process::exit(0);
            }
            // Otherwise the file opens in place; without one, config.json is honoured, falling back
            // to the local file (with a warning) if that location is unavailable.
            let database = match tauri::async_runtime::block_on(db::Db::startup_with_file(config, launch_file)) {
                Ok(database) => database,
                Err(err) => {
                    // Returning an error from setup aborts the process with no explanation (macOS
                    // shows only a crash report). Start instead and let the window explain.
                    eprintln!("LocalBooks could not open its company file: {err}");
                    commands::record_startup_failure(&data_dir, &err);
                    app.manage(commands::StartupFailure { message: err, company_file: data_dir.join(storage::DB_FILE_NAME) });
                    return Ok(());
                }
            };
            tauri::async_runtime::block_on(database.refresh_gate());
            app.manage(database);
            app.manage(simplefin::SimpleFin::new(simplefin::secret_store(&config_dir))?);
            // The local MCP server (off unless turned on in Settings) reads whichever company file is
            // live at the time of each request, and none while its users are signed out.
            let pool_handle = app.handle().clone();
            let pools: mcp_server::PoolSource = std::sync::Arc::new(move || {
                let handle = pool_handle.clone();
                Box::pin(async move { handle.state::<db::Db>().readable_pool().await })
            });
            app.manage(mcp_server::McpService::new(config_dir.clone(), pools));
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
            tauri::async_runtime::spawn(async move { commands::company_changed(&title_handle).await });

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
        .invoke_handler(guarded(tauri::generate_handler![
            commands::list_accounts,
            commands::list_account_types,
            commands::create_account,
            commands::update_account,
            commands::set_account_active,
            commands::list_classes,
            commands::create_class,
            commands::update_class,
            commands::set_class_active,
            commands::delete_class,
            commands::list_reclass_lines,
            commands::get_account_register,
            commands::get_party,
            commands::update_party,
            commands::set_party_active,
            commands::update_product,
            commands::set_product_active,
            commands::list_credit_memos,
            commands::create_credit_memo,
            commands::apply_customer_credit,
            commands::list_customer_credit_applications,
            commands::remove_customer_credit,
            commands::refund_customer_credit,
            commands::list_vendor_credits,
            commands::create_vendor_credit,
            commands::apply_vendor_credit,
            commands::list_vendor_credit_applications,
            commands::remove_vendor_credit,
            commands::refund_vendor_credit,
            commands::reclassify_lines,
            commands::get_dashboard,
            commands::post_journal_entry,
            commands::list_invoices,
            commands::create_invoice,
            commands::record_invoice_payment,
            commands::list_invoice_payments,
            commands::get_ar_aging,
            commands::inspect_bank_file,
            commands::list_inventory_items,
            commands::track_inventory,
            commands::adjust_inventory,
            commands::list_stock_movements,
            commands::preview_fx_revaluation,
            commands::post_fx_revaluation,
            commands::list_fx_revaluations,
            commands::void_fx_revaluation,
            commands::get_estimate_detail,
            commands::invoice_from_estimate,
            commands::import_bank_csv,
            commands::list_bank_transactions,
            commands::simplefin_list,
            commands::simplefin_connect,
            commands::simplefin_reconnect,
            commands::simplefin_refresh_accounts,
            commands::simplefin_link,
            commands::simplefin_unlink,
            commands::simplefin_suggested_start,
            commands::simplefin_sync,
            commands::simplefin_disconnect,
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
            commands::get_profit_and_loss_by_class,
            commands::get_profit_and_loss_periods,
            commands::get_trial_balance_as_of,
            commands::get_cash_flow,
            commands::get_general_ledger,
            commands::get_aging,
            commands::get_summary_report,
            commands::list_tax_rates,
            commands::create_tax_rate,
            commands::update_tax_rate,
            commands::set_tax_rate_active,
            commands::pay_sales_tax,
            commands::list_sales_tax_payments,
            commands::get_tax_liability,
            commands::replace_invoice,
            commands::replace_bill,
            commands::replace_expense,
            commands::replace_journal_entry,
            commands::get_document_lines,
            commands::get_customer_statement,
            commands::list_statement_customers,
            commands::get_1099_report,
            commands::list_recurring,
            commands::save_recurring,
            commands::set_recurring_active,
            commands::delete_recurring,
            commands::enter_recurring,
            commands::list_purchase_orders,
            commands::get_purchase_order_lines,
            commands::create_purchase_order,
            commands::set_purchase_order_closed,
            commands::bill_purchase_order,
            commands::search_books,
            commands::add_attachment,
            commands::list_attachments,
            commands::save_attachment,
            commands::delete_attachment,
            commands::list_budgets,
            commands::create_budget,
            commands::delete_budget,
            commands::get_budget_amounts,
            commands::set_budget_amounts,
            commands::get_budget_vs_actual,
            commands::get_balance_sheet,
            commands::get_balance_sheet_columns,
            commands::get_balance_sheet_budget_vs_actual,
            commands::get_inventory_valuation,
            commands::get_inventory_valuation_detail,
            commands::get_stock_status,
            commands::get_missing_checks,
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
            commands::file_dentist_checkup,
            commands::file_dentist_rebuild,
            commands::propose_forensic_corrections,
            commands::apply_forensic_correction,
            commands::open_company_file,
            commands::get_start_screen,
            commands::forget_recent_company,
            commands::find_company_files,
            commands::create_company_file,
            commands::restore_backup_as,
            commands::close_company_file,
            commands::get_closing_status,
            commands::set_closing_date,
            commands::get_startup_failure,
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
            commands::list_form_styles,
            commands::create_form_style,
            commands::rename_form_style,
            commands::set_default_form_style,
            commands::delete_form_style,
            commands::get_form_template,
            commands::save_form_template,
            commands::reset_form_template,
            commands::preview_form,
            commands::render_form,
            commands::save_form_pdf,
            commands::get_email_draft,
            commands::email_form_with_mail_app,
            commands::email_form_with_smtp,
            commands::get_smtp_settings,
            commands::save_smtp_settings,
            commands::test_smtp_settings,
            commands::send_smtp_test_email,
            commands::get_email_history,
            commands::list_email_statuses,
            commands::set_email_later,
            commands::list_email_queue,
            commands::get_storage_status,
            commands::get_sign_in_state,
            commands::turn_on_users,
            commands::sign_in,
            commands::sign_out,
            commands::sign_in_remembered,
            commands::forget_remembered_sign_in,
            commands::touch_session,
            commands::sign_out_if_idle,
            commands::list_users,
            commands::add_user,
            commands::update_user,
            commands::change_my_password,
            commands::recover_with_recovery_key,
            commands::replace_recovery_key,
            commands::remove_all_users,
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
            commands::list_ledger_batches,
            commands::get_ledger_batch,
            commands::set_ledger_account,
            commands::auto_map_ledger_accounts,
            commands::import_ledger_lists,
            commands::post_ledger_batch,
            commands::reverse_ledger_batch,
            commands::discard_ledger_batch,
            commands::start_over_ledger_batch,
            commands::reverse_qbw_batch,
            commands::open_window,
        ]))
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
                if let Some(mcp) = app.try_state::<mcp_server::McpService>() {
                    mcp.shutdown().await;
                }
                if let Some(db) = app.try_state::<db::Db>() {
                    db.shutdown().await;
                }
            }),
            _ => {}
        });
}

/// Checks the signed-in user's role ([`permissions`]) before handing a command to `handler`; a
/// refused command fails with the reason and never runs.
fn guarded(handler: impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    move |invoke| {
        let command = invoke.message.command();
        let need = permissions::need(command);
        let activity = matches!(need, permissions::Need::Write | permissions::Need::Admin) || command == "touch_session";
        // Without a company file manager (startup failed), nothing is open to protect.
        if let Some(db) = invoke.message.webview().try_state::<db::Db>() {
            if let Err(reason) = db.check(need, activity) {
                if reason == permissions::IDLE_SIGNED_OUT {
                    let app = invoke.message.webview().app_handle().clone();
                    tauri::async_runtime::spawn(async move { commands::sign_out_after_idle(&app).await });
                }
                invoke.resolver.reject(reason);
                return true;
            }
        }
        handler(invoke)
    }
}
