#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Money is integer cents (see `money` and AGENTS.md). Modules that need floats for geometry,
// scoring or foreign file formats allow this lint locally and say why.
#![deny(clippy::float_arithmetic)]

mod accountant_reports;
mod accountants_copy;
mod accounts;
mod app_menu;
mod ach;
mod mac_app_name;
mod approvals;
mod attachment_export;
mod attachments_library;
mod audit_chain;
mod backup;
mod backup_safety;
mod bank;
mod bank_pdf;
mod banking;
mod batch_invoices;
mod billable_costs;
mod bounced_checks;
mod cash_forecast;
mod classes;
mod cleanup;
mod closing;
mod commands;
mod company_setup;
mod company_snapshot;
mod condense;
mod credits;
mod custom_fields;
mod custom_reports;
mod crypto;
mod customer_groups;
mod db;
mod desktop;
mod diagnostics;
mod doc_numbers;
mod document_inbox;
mod duplicates;
mod edits;
mod email_log;
mod envelopes;
mod estimates;
mod expenses;
mod forensic;
mod form_1099;
mod forms;
mod fx_rates;
mod gnucash_import;
mod graphs;
mod guards;
#[cfg(test)]
mod docs_checks;
#[cfg(test)]
mod frontend_checks;
mod iif_import;
mod installed_qb;
mod export_engine;
mod file_cipher;
mod file_menu;
mod file_repair;
mod finance_charges;
mod instance;
mod info_panel;
mod inventory;
mod item_receipts;
mod invoices;
mod item_details;
mod item_categories;
mod item_prices;
mod item_types;
mod job_reports;
mod ledger;
mod list_delete;
mod list_order;
mod list_export;
mod list_reports;
mod mfa;
mod money;
mod mail_app;
mod offsite;
mod opening;
mod other_names;
mod parties;
mod party_contacts;
mod payroll_import;
mod payout_feeds;
mod permissions;
mod pdf;
mod platform_auth;
mod mcp_server;
mod merges;
mod mileage;
mod money_bars;
mod nacha;
mod preferences;
mod price_levels;
mod print_batches;
mod print_queue;
mod profile_lists;
mod purchase_lines;
mod qbmac_import;
mod qbw_backup;
mod qbw_company;
mod qbw_documents;
mod qbw_full_import;
mod qbw_import;
mod qbw_ledger_import;
mod qbw_pages;
mod qbw_records;
mod qbw_stock;
mod qbw_stock_import;
mod qif_import;
mod report_import;
mod reconcile;
mod registers;
mod reminders;
mod report_center;
mod report_comments;
mod release_check;
mod reports;
mod rollback;
mod revaluation;
mod sales;
mod sales_header;
mod sales_lists;
mod sales_orders;
mod sales_prefs;
mod sales_tax;
mod tax_ids;
mod txf;
mod temp_files;
#[cfg(test)]
mod test_companies;
mod secrets;
mod sign_in_throttle;
mod simplefin;
mod smtp;
mod statements;
mod storage;
mod wal_guard;
mod text_import;
mod timesheet;
mod users;
mod vouchers;
mod windows;
mod workflows;
mod xlsx_export;

#[cfg(test)]
mod harness;
mod payment_reminders;
mod report_pdf;
mod report_schedules;
mod statement_charges;
mod sheet_import;

use tauri::Manager;

/// Where the company file, backups and settings live. Development builds (`tauri dev`, debug)
/// keep their own under a `dev-builds` folder, so a build from an older branch can never open,
/// upgrade or lock the books the installed app uses.
pub(crate) fn data_dirs<R: tauri::Runtime>(app: &impl Manager<R>) -> tauri::Result<(std::path::PathBuf, std::path::PathBuf)> {
    let (data, config) = (app.path().app_data_dir()?, app.path().app_config_dir()?);
    if cfg!(debug_assertions) {
        Ok((data.join("dev-builds"), config.join("dev-builds")))
    } else {
        Ok((data, config))
    }
}

fn main() {
    diagnostics::mark_launch();
    // Internal: the release check's headless company-file probe (see release_check.rs).
    if let Some(code) = release_check::run_from_args() {
        std::process::exit(code);
    }
    // A Mac copy updated from LocalBooks takes its new name, before any window (mac_app_name.rs).
    mac_app_name::adopt_new_name();
    tauri::Builder::default()
        .on_page_load(|webview, payload| {
            if webview.label() == windows::MAIN && payload.event() == tauri::webview::PageLoadEvent::Finished {
                diagnostics::first_page_loaded();
                opening::page_loaded();
            }
        })
        .menu(|handle| {
            let menu = tauri::menu::Menu::default(handle)?;
            // Tauri's default Help menu is empty on macOS, so the system fills it with a search box
            // that only finds Apple's own help topics. Drop it there (elsewhere it holds About).
            #[cfg(target_os = "macos")]
            if let Some(help) = menu.get(tauri::menu::HELP_SUBMENU_ID) {
                menu.remove(&help)?;
            }
            file_menu::add_company_items(handle, &menu)?;
            app_menu::install(handle, &menu)?;
            windows::add_menus(handle, &menu)?;
            Ok(menu)
        })
        .plugin(tauri_plugin_dialog::init())
        // Signed updates from the GitHub releases (see plugins.updater in tauri.conf.json).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(windows::OpenWindows::default())
        .on_window_event(windows::on_window_event)
        .on_menu_event(|app, event| {
            windows::on_menu_event(app, event.id().as_ref());
            file_menu::on_menu_event(app, event.id().as_ref());
            app_menu::on_menu_event(app, event.id().as_ref());
        })
        .setup(|app| {
            let mut steps = diagnostics::StartupSteps::start();
            // Before any secret is read: only the installed app uses the system Keychain or
            // Credential Manager; development and test builds keep files in `config_dir`.
            secrets::install_for_this_build(&app.config().identifier);
            let (data_dir, config_dir) = data_dirs(app)?;
            audit_chain::install_anchor_store(&config_dir);
            // Help › Save Diagnostics: a local log of starts, stops and crashes (nothing is sent).
            if let Ok(logs) = app.path().app_log_dir() {
                diagnostics::install(logs);
            }
            diagnostics::log(&format!("Tillhouse {} started", app.package_info().version));
            steps.step("settings and logs");
            // Temporary copies an earlier run left (staged mail, an unpacked backup) go now.
            if let Ok(cache) = app.path().app_cache_dir() {
                temp_files::sweep(&cache, temp_files::KEEP, std::time::SystemTime::now());
            }
            steps.step("clearing temporary files");
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
            steps.step("hand-off check");
            // The company file opens off the UI thread (gap R28): the window paints at once and shows
            // Opening <name>… until it is open. Meanwhile the books stand empty, and the access gate
            // stays unchecked, so only the commands that are safe before sign-in run.
            app.manage(opening::Opening::default());
            opening::install(app.handle());
            app.manage(db::Db::closed(config.clone()));
            // Announced now, before the window can ask and before a second launch can hand anything
            // over, so the window never starts on No Company Open by mistake.
            let target = launch_target(&config, launch_file.as_deref());
            let turn = opening::begin_at_launch(app.handle(), target.as_deref().map(|path| (path, opening::recent_name(&config_dir, path))));
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

            // Accept hand-offs from later launches, before the open: a second launch during a slow
            // open hands its file over (it opens once this one is done) instead of starting another
            // LocalBooks that would fight over the file.
            let handoff_handle = app.handle().clone();
            let guard = tauri::async_runtime::block_on(instance::listen(&config_dir, move |request| {
                let app = handoff_handle.clone();
                tauri::async_runtime::spawn(async move { commands::handle_hand_off(&app, request).await });
            }));
            steps.step("services and hand-off listener");
            match guard {
                Ok(guard) => {
                    app.manage(std::sync::Mutex::new(Some(guard)));
                }
                Err(err) => eprintln!("single-instance hand-off unavailable: {err}"),
            }
            steps.finish("services and hand-off listener");

            // With a file, it opens in place; without one, config.json is honoured, falling back to
            // the local file (with a warning) if that location is unavailable.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { open_at_launch(handle, config, launch_file, data_dir, turn).await });
            Ok(())
        })
        .invoke_handler(guarded(commands::handler()))
        .build(tauri::generate_context!())
        .expect("error while building Tillhouse")
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
                diagnostics::log("Tillhouse quit");
                // Quitting closes the company: its desktop is saved while the windows are still known.
                windows::save_on_close(app).await;
                if let Some(state) = app.try_state::<std::sync::Mutex<Option<instance::InstanceGuard>>>() {
                    drop(state.lock().ok().and_then(|mut guard| guard.take()));
                }
                if let Some(mcp) = app.try_state::<mcp_server::McpService>() {
                    mcp.shutdown().await;
                }
                if let Some(db) = app.try_state::<db::Db>() {
                    db.shutdown().await;
                }
                if let Ok(cache) = app.path().app_cache_dir() {
                    temp_files::sweep(&cache, temp_files::KEEP, std::time::SystemTime::now());
                }
            }),
            _ => {}
        });
}

/// The launch's company file, opened off the UI thread (gap R28; `docs/plans/opening-screen.md`).
/// The window shows Opening <name>… until this ends; then it reloads into the failure screen,
/// sign-in or the app. The work that needs the books open starts only once they are.
async fn open_at_launch(app: tauri::AppHandle, config: db::DbConfig, launch_file: Option<std::path::PathBuf>, data_dir: std::path::PathBuf, turn: opening::Turn) {
    let started = std::time::Instant::now();
    let opened = db::Db::startup_with_file(config, launch_file).await;
    diagnostics::log(&format!("company file opened in {} ms{}", started.elapsed().as_millis(), if opened.is_err() { " (failed)" } else { "" }));
    let database = app.state::<db::Db>();
    match opened {
        Ok(opened) => {
            database.adopt(opened).await;
            database.refresh_gate().await;
        }
        Err(err) => {
            // The window explains (StartupFailure.jsx); managed before the turn ends, so the window
            // finds it when it reloads.
            eprintln!("Tillhouse could not open its company file: {err}");
            commands::record_startup_failure(&data_dir, &err);
            app.manage(commands::StartupFailure { message: err, company_file: data_dir.join(storage::DB_FILE_NAME) });
            drop(turn);
            return;
        }
    }
    // The first window to load reopens the company's saved desktop (Desktop View preferences).
    windows::restore_on_next_load(&app, true);
    commands::company_changed(&app).await;
    drop(turn);
    start_background_work(&app);
}

/// Which file the launch is about to open, to name it on screen (`Db::startup_with_file` decides
/// the same way, and may still fall back to the local file). `None`: the company was left closed,
/// so nothing opens and the window goes to No Company Open.
fn launch_target(config: &db::DbConfig, launch_file: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    if let Some(file) = launch_file {
        return Some(file.to_path_buf());
    }
    let saved = storage::read_config(&config.config_dir);
    if saved.start_with_no_company {
        return None;
    }
    Some(saved.database_directory.map_or_else(|| config.default_path.clone(), |dir| storage::resolve_network_path(&dir).join(storage::DB_FILE_NAME)))
}

/// What runs while LocalBooks stays open, once the launch's company file is: scheduled reports,
/// payout feeds, idle sign-out, the daily backup and offsite copies.
fn start_background_work(app: &tauri::AppHandle) {
    // Scheduled reports: due ones run soon after launch (catching up on runs missed while
    // LocalBooks was closed), then every few minutes while it stays open.
    let schedules = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            commands::run_due_schedules(&schedules).await;
            tokio::time::sleep(std::time::Duration::from_secs(10 * 60)).await;
        }
    });

    // Payout feeds an Admin set to fetch automatically: checked every 15 minutes while the
    // app is open, each fetched when its interval has passed (`payout_feeds::fetch_due`).
    let payouts = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        loop {
            commands::run_due_payout_fetches(&payouts).await;
            tokio::time::sleep(std::time::Duration::from_secs(15 * 60)).await;
        }
    });

    // Idle sign-out even when no window asks: once a minute, so an encrypted company file
    // (gap R1) is closed, and its key leaves memory, soon after the idle limit passes.
    let idle_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            if idle_handle.state::<db::Db>().idle_expired() {
                commands::sign_out_after_idle(&idle_handle).await;
            }
        }
    });

    // Daily automatic backup, off the startup path.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let db = handle.state::<db::Db>();
        if let Err(err) = backup::auto_backup(db.inner()).await {
            eprintln!("automatic backup failed: {err}");
        }
        // Offsite copies while the app stays open: checked now and every hour, made when
        // the newest is older than a day (`offsite::INTERVAL`). A failure is kept on the
        // Offsite screen; the message never holds the key or password.
        loop {
            if let Ok(store) = commands::offsite_secrets(&handle) {
                let _ = offsite::run_if_due(db.inner(), &store).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
        }
    });
}

/// Checks the signed-in user's role ([`permissions`]) before handing a command to `handler`; a
/// refused command fails with the reason and never runs.
fn guarded(handler: impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    move |invoke| {
        let command = invoke.message.command();
        let need = permissions::need(command);
        let activity = matches!(need, permissions::Need::Write | permissions::Need::Admin) || command == "touch_session";
        // When the launch could not open the company file, nothing is open to protect, and the
        // failure screen's own commands (the updater's) must run.
        let failed = invoke.message.webview().try_state::<commands::StartupFailure>().is_some();
        if let Some(db) = invoke.message.webview().try_state::<db::Db>().filter(|_| !failed) {
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
