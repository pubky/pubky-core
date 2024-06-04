use anyhow::anyhow;
use lazy_static::lazy_static;
use std::sync::Arc;
use tauri::Manager;

use shared::{App, Core, Effect, Event};

lazy_static! {
    static ref CORE: Arc<Core<Effect, App>> = Arc::new(Core::new());
}

fn handle_event(
    event: Event,
    core: &Arc<Core<Effect, App>>,
    tauri_app: tauri::AppHandle,
) -> anyhow::Result<()> {
    for effect in core.process_event(event) {
        process_effect(effect, core, tauri_app.clone())?
    }

    Ok(())
}

fn process_effect(
    effect: Effect,
    core: &Arc<Core<Effect, App>>,
    tauri_app: tauri::AppHandle,
) -> anyhow::Result<()> {
    match effect {
        Effect::Render(_) => {
            let view = core.view();
            tauri_app.emit_all("render", view).map_err(|e| anyhow!(e))
        }
    }
}

/// The main entry point for Tauri
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // .invoke_handler(tauri::generate_handler![increment, decrement, watch])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
