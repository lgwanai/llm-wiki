//! llm-wiki Tauri desktop application.

mod commands;

use commands::AppState;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{Emitter, Manager};

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::set_project_path,
            commands::get_wiki_status,
            commands::get_wiki_pages,
            commands::get_page_content,
            commands::get_source_file_content,
            commands::save_source_file_content,
            commands::get_graph_data,
            commands::search_wiki,
            commands::run_lint,
            commands::list_source_files,
            commands::import_file,
            commands::compile_source_file,
            commands::chat_query,
            commands::check_config,
            commands::save_config,
            commands::list_ledger_tables,
            commands::get_table_content,
            commands::save_page_content,
            commands::get_full_config,
            commands::open_settings_window,
        ])
        .setup(|app| {
            let menu = MenuBuilder::new(app)
                // ── App Menu ──
                .item(
                    &SubmenuBuilder::new(app, "llm-wiki")
                        .item(&PredefinedMenuItem::about(
                            app,
                            Some("About llm-wiki"),
                            None,
                        )?)
                        .separator()
                        .item(
                            &MenuItemBuilder::with_id("settings", "Settings...")
                                .accelerator("CmdOrCtrl+,")
                                .build(app)?,
                        )
                        .separator()
                        .item(&PredefinedMenuItem::services(app, None)?)
                        .separator()
                        .item(&PredefinedMenuItem::hide(app, Some("Hide llm-wiki"))?)
                        .item(&PredefinedMenuItem::hide_others(app, Some("Hide Others"))?)
                        .item(&PredefinedMenuItem::show_all(app, Some("Show All"))?)
                        .separator()
                        .item(&PredefinedMenuItem::quit(app, Some("Quit llm-wiki"))?)
                        .build()?,
                )
                // ── File Menu ──
                .item(
                    &SubmenuBuilder::new(app, "File")
                        .item(
                            &MenuItemBuilder::with_id("open_workspace", "Open Workspace...")
                                .accelerator("CmdOrCtrl+O")
                                .build(app)?,
                        )
                        .item(
                            &MenuItemBuilder::with_id("import_files", "Import Files...")
                                .accelerator("CmdOrCtrl+I")
                                .build(app)?,
                        )
                        .separator()
                        .item(
                            &MenuItemBuilder::with_id("compile_all", "Compile All")
                                .accelerator("CmdOrCtrl+Shift+C")
                                .build(app)?,
                        )
                        .separator()
                        .item(&PredefinedMenuItem::close_window(
                            app,
                            Some("Close Window"),
                        )?)
                        .build()?,
                )
                // ── Edit Menu ──
                .item(
                    &SubmenuBuilder::new(app, "Edit")
                        .item(&PredefinedMenuItem::undo(app, Some("Undo"))?)
                        .item(&PredefinedMenuItem::redo(app, Some("Redo"))?)
                        .separator()
                        .item(&PredefinedMenuItem::cut(app, Some("Cut"))?)
                        .item(&PredefinedMenuItem::copy(app, Some("Copy"))?)
                        .item(&PredefinedMenuItem::paste(app, Some("Paste"))?)
                        .item(&PredefinedMenuItem::select_all(app, Some("Select All"))?)
                        .build()?,
                )
                // ── View Menu ──
                .item(
                    &SubmenuBuilder::new(app, "View")
                        .item(
                            &MenuItemBuilder::with_id("toggle_files", "Show Files Panel")
                                .accelerator("CmdOrCtrl+1")
                                .build(app)?,
                        )
                        .item(
                            &MenuItemBuilder::with_id("toggle_graph", "Show Graph Panel")
                                .accelerator("CmdOrCtrl+2")
                                .build(app)?,
                        )
                        .item(
                            &MenuItemBuilder::with_id("toggle_tables", "Show Tables Panel")
                                .accelerator("CmdOrCtrl+3")
                                .build(app)?,
                        )
                        .separator()
                        .item(
                            &MenuItemBuilder::with_id("toggle_chat", "Show Chat Panel")
                                .accelerator("CmdOrCtrl+4")
                                .build(app)?,
                        )
                        .separator()
                        .item(
                            &MenuItemBuilder::with_id("reload", "Reload")
                                .accelerator("CmdOrCtrl+R")
                                .build(app)?,
                        )
                        .build()?,
                )
                // ── Help Menu ──
                .item(
                    &SubmenuBuilder::new(app, "Help")
                        .item(
                            &MenuItemBuilder::with_id("lint", "Run Wiki Health Check")
                                .build(app)?,
                        )
                        .build()?,
                )
                .build()?;
            app.set_menu(menu)?;

            // ── Menu event handler ──
            let handle = app.handle().clone();
            app.on_menu_event(move |app, event| {
                let id = event.id().0.as_str();
                match id {
                    "settings" => {
                        // Open settings as a separate window
                        let _ = tauri::WebviewWindowBuilder::new(
                            app,
                            "settings",
                            tauri::WebviewUrl::App("public/settings.html".into()),
                        )
                        .title("Settings — llm-wiki")
                        .inner_size(600.0, 520.0)
                        .resizable(true)
                        .center()
                        .build();
                    }
                    "open_workspace" | "import_files" | "compile_all" | "toggle_files"
                    | "toggle_graph" | "toggle_tables" | "toggle_chat" | "reload" | "lint" => {
                        let _ = handle.emit("menu-action", id);
                    }
                    _ => {}
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
