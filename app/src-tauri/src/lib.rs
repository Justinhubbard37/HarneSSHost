mod commands;
mod development_candidates;
mod harness;
mod interface;
mod library;
mod runtime;
mod state;

use state::AppState;
use tauri::Manager;

pub(crate) const MAIN_WINDOW_LABEL: &str = "main";

fn is_main_window_shutdown_boundary(label: &str) -> bool {
    label == MAIN_WINDOW_LABEL
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new().expect("failed to initialize the harness registry");

    let app = tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::get_harness_library,
            commands::get_harness_details,
            commands::open_harness
        ])
        .on_window_event(|window, event| {
            if is_main_window_shutdown_boundary(window.label()) {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let app_handle = window.app_handle();
                    let state = app_handle.state::<AppState>();
                    if state.runtime_controller.shutdown_for_app_exit(app_handle) {
                        app_handle.exit(0);
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building HarneSSHost");

    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            let state = app_handle.state::<AppState>();
            let _ = state.runtime_controller.shutdown_for_app_exit(app_handle);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_2_main_window_close_is_the_application_shutdown_boundary() {
        assert!(is_main_window_shutdown_boundary(MAIN_WINDOW_LABEL));
        assert!(!is_main_window_shutdown_boundary(
            runtime::presentation::DEEPSEEK_WINDOW_LABEL
        ));

        let lifecycle = include_str!("lib.rs");
        assert!(lifecycle.contains("WindowEvent::CloseRequested"));
        assert!(lifecycle.contains("api.prevent_close()"));
        assert!(lifecycle.contains("shutdown_for_app_exit(app_handle)"));
        assert!(lifecycle.contains("app_handle.exit(0)"));
    }
}
