#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod ai;
use serde::Serialize;
use spacelens_core::{model::*, platform, query, store, Engine};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

struct AppState {
    engine: Arc<Engine>,
    scopes: Mutex<HashMap<String, Scope>>,
    home: PathBuf,
    exit_pending: AtomicBool,
}
#[derive(Serialize)]
struct Bootstrap {
    scans: Vec<Scan>,
    scopes: Vec<Scope>,
    display_scan_id: Option<String>,
    active_scan_id: Option<String>,
    settings: Settings,
    home_scope: Option<Scope>,
}
#[derive(Serialize)]
struct Summary {
    scan: Scan,
    root: Entry,
}
async fn db_query<T: Send + 'static>(
    path: PathBuf,
    f: impl FnOnce(spacelens_core::DbConnection) -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = store::connect(&path)?;
        f(db)
    })
    .await
    .map_err(|_| AppError::new("query_failed", "查询线程未正常完成。"))?
}
#[tauri::command]
async fn get_app_state(state: State<'_, AppState>) -> Result<Bootstrap> {
    let path = state.engine.database.clone();
    let (scans, display_scan_id, settings) = db_query(path, |db| {
        Ok((
            store::scans(&db)?,
            store::display_id(&db)?,
            store::settings(&db)?,
        ))
    })
    .await?;
    let scopes = state
        .scopes
        .lock()
        .unwrap()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let home_scope = scopes.iter().find(|s| s.path == state.home).cloned();
    Ok(Bootstrap {
        scans,
        scopes,
        display_scan_id,
        active_scan_id: state.engine.active_id(),
        settings,
        home_scope,
    })
}
#[tauri::command]
async fn list_volumes(state: State<'_, AppState>) -> Result<Vec<Volume>> {
    let home = state.home.clone();
    tauri::async_runtime::spawn_blocking(move || platform::volumes(&home))
        .await
        .map_err(|_| AppError::new("volume_error", "无法读取卷信息。"))
}
#[tauri::command]
async fn choose_scope(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    kind: String,
    volume_id: Option<String>,
) -> Result<Option<Scope>> {
    if state.engine.active_id().is_some() {
        return Err(AppError::new("scan_busy", "扫描时不能更换范围。"));
    }
    let path = match kind.as_str() {
        "home" => Some(state.home.clone()),
        "volume" => platform::volumes(&state.home)
            .into_iter()
            .find(|v| Some(&v.id) == volume_id.as_ref())
            .map(|v| PathBuf::from(v.path_display)),
        "directory" => tauri::async_runtime::spawn_blocking(move || {
            app.dialog()
                .file()
                .set_title("选择要分析的文件夹")
                .blocking_pick_folder()
                .and_then(|p| p.into_path().ok())
        })
        .await
        .map_err(|_| AppError::new("scope_invalid", "目录选择器未完成。"))?,
        _ => return Err(AppError::new("scope_invalid", "无效范围类型。")),
    };
    let Some(path) = path else { return Ok(None) };
    let scope = platform::scope(path)?;
    if state.engine.active_id().is_some() {
        return Err(AppError::new("scan_busy", "已有扫描开始，请稍后更换范围。"));
    }
    state
        .scopes
        .lock()
        .unwrap()
        .insert(scope.id.clone(), scope.clone());
    Ok(Some(scope))
}
#[tauri::command]
async fn start_scan(
    state: State<'_, AppState>,
    scope_id: String,
    request_id: String,
) -> Result<Scan> {
    let scope = state
        .scopes
        .lock()
        .unwrap()
        .get(&scope_id)
        .cloned()
        .ok_or_else(|| AppError::new("scope_invalid", "请重新选择扫描范围。"))?;
    let engine = state.engine.clone();
    tauri::async_runtime::spawn_blocking(move || engine.start(scope, &request_id))
        .await
        .map_err(|_| AppError::new("scan_failed", "扫描未能启动。"))?
}
#[tauri::command]
async fn cancel_scan(state: State<'_, AppState>, scan_id: String) -> Result<Scan> {
    let engine = state.engine.clone();
    tauri::async_runtime::spawn_blocking(move || engine.cancel(&scan_id))
        .await
        .map_err(|_| AppError::new("scan_failed", "取消请求未能完成。"))?
}
#[tauri::command]
async fn get_scan_status(state: State<'_, AppState>, scan_id: String) -> Result<Scan> {
    db_query(state.engine.database.clone(), move |db| {
        store::scan(&db, &scan_id)
    })
    .await
}
#[tauri::command]
async fn get_result_summary(state: State<'_, AppState>, scan_id: String) -> Result<Summary> {
    db_query(state.engine.database.clone(), move |db| {
        Ok(Summary {
            scan: store::require_result(&db, &scan_id)?,
            root: store::entry(&db, &scan_id, 1)?,
        })
    })
    .await
}
#[tauri::command]
async fn list_children(state: State<'_, AppState>, query: Query) -> Result<Page<Entry>> {
    db_query(state.engine.database.clone(), move |db| {
        query::list(&db, &query, "children")
    })
    .await
}
#[tauri::command]
async fn list_large_files(state: State<'_, AppState>, query: Query) -> Result<Page<Entry>> {
    db_query(state.engine.database.clone(), move |db| {
        query::list(&db, &query, "large")
    })
    .await
}
#[tauri::command]
async fn list_entry_aliases(state: State<'_, AppState>, query: Query) -> Result<Page<Entry>> {
    db_query(state.engine.database.clone(), move |db| {
        query::list(&db, &query, "aliases")
    })
    .await
}
#[tauri::command]
async fn get_directory_map(
    state: State<'_, AppState>,
    scan_id: String,
    parent_id: String,
    metric: Metric,
) -> Result<DirectoryMap> {
    db_query(state.engine.database.clone(), move |db| {
        query::map(&db, &scan_id, &parent_id, metric)
    })
    .await
}
#[tauri::command]
async fn get_entry_detail(
    state: State<'_, AppState>,
    scan_id: String,
    entry_id: String,
) -> Result<Detail> {
    db_query(state.engine.database.clone(), move |db| {
        query::detail(&db, &scan_id, &entry_id)
    })
    .await
}
#[tauri::command]
async fn list_scan_errors(
    state: State<'_, AppState>,
    scan_id: String,
    cursor: Option<String>,
) -> Result<Page<ScanError>> {
    db_query(state.engine.database.clone(), move |db| {
        query::errors(&db, &scan_id, cursor)
    })
    .await
}
#[tauri::command]
async fn show_result(state: State<'_, AppState>, scan_id: String) -> Result<()> {
    db_query(state.engine.database.clone(), move |db| {
        store::show(&db, &scan_id)
    })
    .await
}
#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<Settings> {
    db_query(state.engine.database.clone(), |db| store::settings(&db)).await
}
#[tauri::command]
async fn update_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    db_query(state.engine.database.clone(), move |mut db| {
        store::save_settings(&mut db, &settings)
    })
    .await
}
#[tauri::command]
async fn copy_entry_path(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    scan_id: String,
    entry_id: String,
) -> Result<bool> {
    let (_, raw, _) = db_query(state.engine.database.clone(), move |db| {
        store::native_path(&db, &scan_id, &entry_id)
    })
    .await?;
    let lossy = std::str::from_utf8(&raw).is_err();
    let path = platform::from_bytes(raw);
    app.clipboard()
        .write_text(path.to_string_lossy().into_owned())
        .map_err(|_| AppError::new("clipboard_error", "无法写入剪贴板。"))?;
    Ok(lossy)
}
#[tauri::command]
async fn reveal_entry(state: State<'_, AppState>, scan_id: String, entry_id: String) -> Result<()> {
    db_query(state.engine.database.clone(), move |db| {
        let (root, raw, identity) = store::native_path(&db, &scan_id, &entry_id)?;
        let path = platform::verified_path(&root, raw, identity)?;
        let status = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(path)
            .status()?;
        if !status.success() {
            return Err(AppError::new("reveal_failed", "Finder 未能定位这个项目。"));
        }
        Ok(())
    })
    .await
}
#[tauri::command]
fn open_permission_settings() -> Result<()> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .status()?;
    if !status.success() {
        return Err(AppError::new(
            "settings_unavailable",
            "请手动打开系统设置 → 隐私与安全性 → 完全磁盘访问权限。",
        ));
    }
    Ok(())
}
#[tauri::command]
fn quit_after_cancel(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    if state.exit_pending.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    if let Some(id) = state.engine.active_id() {
        state.engine.cancel(&id)?;
    }
    let engine = state.engine.clone();
    std::thread::spawn(move || {
        while engine.active_id().is_some() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        app.exit(0);
    });
    Ok(())
}
fn main() {
    tauri::Builder::default()
        .manage(ai::AiState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700))?;
            let handle = app.handle().clone();
            let engine = Engine::new(data.join("index.sqlite"), vec![data], move |p| {
                let _ = handle.emit("scan_progress", p);
            })?;
            let home = app.path().home_dir()?;
            let db = store::connect(&engine.database)?;
            let mut scopes = store::restore_scopes(&db)?;
            if !scopes.iter().any(|s| s.path == home) {
                if let Ok(s) = platform::scope(home.clone()) {
                    scopes.push(s);
                }
            }
            app.manage(AppState {
                engine,
                scopes: Mutex::new(scopes.into_iter().map(|s| (s.id.clone(), s)).collect()),
                home,
                exit_pending: AtomicBool::new(false),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.engine.active_id().is_some() {
                    api.prevent_close();
                    let _ = window.emit("exit_requested", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ai::get_ai_settings,
            ai::save_deepseek_key,
            ai::remove_deepseek_key,
            ai::prepare_ai_explanation,
            ai::explain_entry,
            get_app_state,
            list_volumes,
            choose_scope,
            start_scan,
            cancel_scan,
            get_scan_status,
            get_result_summary,
            list_children,
            list_large_files,
            list_entry_aliases,
            get_directory_map,
            get_entry_detail,
            list_scan_errors,
            show_result,
            get_settings,
            update_settings,
            copy_entry_path,
            reveal_entry,
            open_permission_settings,
            quit_after_cancel
        ])
        .build(tauri::generate_context!())
        .expect("无法初始化 SpaceLens")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                let state = app.state::<AppState>();
                if state.engine.active_id().is_some() {
                    api.prevent_exit();
                    let _ = app.emit("exit_requested", ());
                }
            }
        });
}
