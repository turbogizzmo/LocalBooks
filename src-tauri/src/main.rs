#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accountant_reports;
mod accounts;
mod app_menu;
mod ach;
mod audit_chain;
mod backup;
mod bank;
mod bank_pdf;
mod banking;
mod classes;
mod closing;
mod commands;
mod company_setup;
mod credits;
mod custom_fields;
mod custom_reports;
mod crypto;
mod db;
mod edits;
mod email_log;
mod estimates;
mod expenses;
mod forensic;
mod forms;
mod fx_rates;
mod gnucash_import;
#[cfg(test)]
mod docs_checks;
#[cfg(test)]
mod frontend_checks;
mod iif_import;
mod export_engine;
mod file_dentist;
mod file_menu;
mod finance_charges;
mod instance;
mod inventory;
mod invoices;
mod item_prices;
mod job_reports;
mod ledger;
mod mail_app;
mod parties;
mod permissions;
mod pdf;
mod mcp_server;
mod nacha;
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
mod report_import;
mod reconcile;
mod registers;
mod reminders;
mod report_center;
mod release_check;
mod reports;
mod revaluation;
mod sales;
mod sales_lists;
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
mod xlsx_export;

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
    // Internal: the release check's headless company-file probe (see release_check.rs).
    if let Some(code) = release_check::run_from_args() {
        std::process::exit(code);
    }
    tauri::Builder::default()
        .menu(|handle| {
            let menu = tauri::menu::Menu::default(handle)?;
            // Tauri's default Help menu is empty on macOS, so the system fills it with a search box
            // that only finds Apple's own help topics. Drop it there (elsewhere it holds About).
            #[cfg(target_os = "macos")]
            if let Some(help) = menu.get(tauri::menu::HELP_SUBMENU_ID) {
                menu.remove(&help)?;
            }
            windows::add_new_window_item(handle, &menu)?;
            file_menu::add_company_items(handle, &menu)?;
            app_menu::install(handle, &menu)?;
            Ok(menu)
        })
        .plugin(tauri_plugin_dialog::init())
        // Signed updates from the GitHub releases (see plugins.updater in tauri.conf.json).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .on_menu_event(|app, event| {
            windows::on_menu_event(app, event.id().as_ref());
            file_menu::on_menu_event(app, event.id().as_ref());
            app_menu::on_menu_event(app, event.id().as_ref());
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
            app.manage(fx_rates::ReqwestHttp::new()?);
            app.manage(bank::suggest::Suggester::default());
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
        .invoke_handler(guarded(commands::handler()))
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
