#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{
    State, WebviewUrl, WebviewWindowBuilder,
    menu::MenuBuilder,
};

#[derive(Default)]
struct CancellationRegistry(Mutex<HashMap<String, Arc<AtomicBool>>>);

impl CancellationRegistry {
    fn register(&self, request_id: &str) -> Result<Arc<AtomicBool>, String> {
        let mut jobs = self.0.lock().map_err(|_| "registry unavailable")?;
        if jobs.contains_key(request_id) {
            return Err("requestId is already active".into());
        }
        if jobs.len() >= 16 {
            return Err("too many active probes".into());
        }
        let flag = Arc::new(AtomicBool::new(false));
        jobs.insert(request_id.to_owned(), Arc::clone(&flag));
        Ok(flag)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeOutcome {
    request_id: String,
    completed_steps: u16,
    cancelled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShellMetadata {
    tauri_version: &'static str,
    os: &'static str,
    architecture: &'static str,
    privileged_commands_exposed: u8,
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[tauri::command]
fn shell_metadata() -> ShellMetadata {
    ShellMetadata {
        tauri_version: tauri::VERSION,
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        privileged_commands_exposed: 0,
    }
}

#[tauri::command]
async fn cancellable_probe(
    request_id: String,
    steps: u16,
    registry: State<'_, CancellationRegistry>,
) -> Result<ProbeOutcome, String> {
    if !valid_request_id(&request_id) {
        return Err("requestId must contain 1-64 ASCII letters, digits, '-' or '_'".into());
    }
    if !(1..=400).contains(&steps) {
        return Err("steps must be between 1 and 400".into());
    }

    let cancellation = registry.register(&request_id)?;

    let mut completed_steps = 0;
    let mut cancelled = false;
    for _ in 0..steps {
        if cancellation.load(Ordering::Acquire) {
            cancelled = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        completed_steps += 1;
    }

    registry
        .0
        .lock()
        .map_err(|_| "registry unavailable")?
        .remove(&request_id);

    Ok(ProbeOutcome {
        request_id,
        completed_steps,
        cancelled,
    })
}

#[tauri::command]
fn cancel_probe(
    request_id: String,
    registry: State<'_, CancellationRegistry>,
) -> Result<bool, String> {
    if !valid_request_id(&request_id) {
        return Err("invalid requestId".into());
    }
    let jobs = registry.0.lock().map_err(|_| "registry unavailable")?;
    Ok(jobs.get(&request_id).is_some_and(|flag| {
        flag.store(true, Ordering::Release);
        true
    }))
}

fn navigation_is_local(url: &tauri::Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return false;
    }
    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost"))
        || (url.scheme() == "about" && url.path() == "blank")
}

fn main() {
    tauri::Builder::default()
        .manage(CancellationRegistry::default())
        .setup(|app| {
            let menu = MenuBuilder::new(app)
                .text("gate-a-status", "Gate A shell probe")
                .separator()
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;
            app.set_menu(menu)?;

            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("ProjectOS · Gate A shell probe")
                .inner_size(1180.0, 820.0)
                .min_inner_size(720.0, 560.0)
                .on_navigation(navigation_is_local)
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            shell_metadata,
            cancellable_probe,
            cancel_probe
        ])
        .run(tauri::generate_context!())
        .expect("ProjectOS Gate A shell failed to run");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_request_preserves_original_cancellation_handle() {
        let registry = CancellationRegistry::default();
        let original = registry.register("request_1").unwrap();
        assert!(registry.register("request_1").is_err());
        let jobs = registry.0.lock().unwrap();
        assert!(Arc::ptr_eq(jobs.get("request_1").unwrap(), &original));
        jobs.get("request_1").unwrap().store(true, Ordering::Release);
        assert!(original.load(Ordering::Acquire));
    }

    #[test]
    fn active_probe_count_is_bounded() {
        let registry = CancellationRegistry::default();
        for n in 0..16 { registry.register(&format!("r{n}")).unwrap(); }
        assert!(registry.register("overflow").is_err());
    }

    #[test]
    fn navigation_rejects_remote_origins_and_credentials() {
        for url in ["https://attacker.invalid", "file:///etc/passwd", "http://localhost:8000", "javascript:alert(1)", "tauri://user:secret@localhost", "http://user:secret@tauri.localhost", "http://tauri.localhost:8000"] {
            assert!(!navigation_is_local(&tauri::Url::parse(url).unwrap()));
        }
    }
}
