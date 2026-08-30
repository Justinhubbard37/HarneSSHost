use crate::library::{
    build_harness_details, build_harness_library, HarnessDetailsDto, HarnessLibraryDto,
};
use crate::runtime::controller::OpenHarnessResultDto;
use crate::state::AppState;

#[tauri::command]
pub(crate) fn get_harness_library(state: tauri::State<'_, AppState>) -> HarnessLibraryDto {
    build_harness_library(state.inner())
}

#[tauri::command]
pub(crate) fn get_harness_details(
    harness_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<HarnessDetailsDto, String> {
    build_harness_details(state.inner(), &harness_id).map_err(|error| error.code().to_string())
}

#[tauri::command]
pub(crate) async fn open_harness(
    harness_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<OpenHarnessResultDto, String> {
    state
        .runtime_controller
        .open_harness(&app, &harness_id)
        .map_err(str::to_string)
}
