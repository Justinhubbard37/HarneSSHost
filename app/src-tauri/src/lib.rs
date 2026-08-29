mod commands;
mod development_candidates;
mod harness;
mod interface;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new().expect("failed to initialize the harness registry");

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![commands::get_host_snapshot])
        .run(tauri::generate_context!())
        .expect("error while running HarneSSHost");
}
