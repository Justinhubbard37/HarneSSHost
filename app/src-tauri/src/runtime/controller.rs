use crate::harness::adapter::HarnessId;
use crate::interface::{HostSurfaceState, InterfaceFacts, InterfaceResolver};
use crate::runtime::domain::RuntimePhase;
use crate::runtime::driver::{
    HarnessRuntimeDriver, OwnedRuntimeIdentity, PresentationCloseSemantics, RuntimeCompletion,
    RuntimeControl, RuntimeDriverMetadata, RuntimeEventSink, RuntimeFailure,
    RuntimeGenerationReporter, RuntimeRunContext,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
use tauri::Emitter;

pub(crate) const RUNTIME_CHANGED_EVENT: &str = "harness-runtime-changed";
const APP_EXIT_WAIT: Duration = Duration::from_secs(12);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeSnapshot {
    harness_id: HarnessId,
    metadata: RuntimeDriverMetadata,
    phase: RuntimePhase,
    generation: u64,
    cleanup_complete: bool,
    failure_code: Option<&'static str>,
}

impl RuntimeSnapshot {
    pub(crate) fn harness_id(&self) -> &HarnessId {
        &self.harness_id
    }

    pub(crate) fn metadata(&self) -> RuntimeDriverMetadata {
        self.metadata
    }

    pub(crate) fn phase(&self) -> RuntimePhase {
        self.phase
    }

    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn can_open(&self) -> bool {
        self.phase == RuntimePhase::Inactive
            || (self.phase == RuntimePhase::Failed && self.cleanup_complete)
            || self.phase == RuntimePhase::Ready
    }

    pub(crate) fn failure_code(&self) -> Option<&'static str> {
        self.failure_code
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenHarnessResultDto {
    harness_id: String,
    phase: RuntimePhase,
    can_open: bool,
    surface: HostSurfaceState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeFailureDto {
    harness_id: String,
    code: String,
}

impl RuntimeFailureDto {
    fn new(harness_id: &HarnessId, code: &'static str) -> Self {
        Self {
            harness_id: harness_id.as_str().to_string(),
            code: code.to_string(),
        }
    }
}

impl From<RuntimeFailure> for RuntimeFailureDto {
    fn from(failure: RuntimeFailure) -> Self {
        Self::new(failure.harness_id(), failure.code())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeChangedEventDto {
    harness_id: String,
    phase: RuntimePhase,
    can_open: bool,
    surface: HostSurfaceState,
}

#[derive(Clone)]
pub(crate) struct RuntimeController {
    shared: Arc<ControllerShared>,
    drivers: Arc<BTreeMap<HarnessId, Arc<dyn HarnessRuntimeDriver>>>,
    resolver: Arc<dyn InterfaceResolver>,
}

struct ControllerShared {
    state: Mutex<ControllerState>,
    changed: Condvar,
}

struct ControllerState {
    runtimes: BTreeMap<HarnessId, RuntimeEntryState>,
}

struct RuntimeEntryState {
    phase: RuntimePhase,
    generation: u64,
    cleanup_complete: bool,
    command_sender: Option<mpsc::Sender<RuntimeControl>>,
    failure_code: Option<&'static str>,
    shutdown_requested: bool,
}

impl Default for RuntimeEntryState {
    fn default() -> Self {
        Self {
            phase: RuntimePhase::Inactive,
            generation: 0,
            cleanup_complete: true,
            command_sender: None,
            failure_code: None,
            shutdown_requested: false,
        }
    }
}

enum OpenDirective {
    Launch {
        generation: u64,
        commands: mpsc::Receiver<RuntimeControl>,
    },
    Focus {
        generation: u64,
    },
    Reuse,
}

impl ControllerState {
    fn new(harness_ids: impl IntoIterator<Item = HarnessId>) -> Self {
        Self {
            runtimes: harness_ids
                .into_iter()
                .map(|harness_id| (harness_id, RuntimeEntryState::default()))
                .collect(),
        }
    }

    fn begin_open(&mut self, harness_id: &HarnessId) -> Result<OpenDirective, &'static str> {
        let entry = self
            .runtimes
            .get(harness_id)
            .ok_or("harness.runtime-not-available")?;
        let would_launch = entry.phase == RuntimePhase::Inactive
            || (entry.phase == RuntimePhase::Failed && entry.cleanup_complete);
        if would_launch
            && self.runtimes.iter().any(|(other_id, other)| {
                other_id != harness_id && other.blocks_another_runtime_launch()
            })
        {
            return Err("harness.runtime-busy");
        }
        self.runtimes
            .get_mut(harness_id)
            .expect("runtime identity was checked")
            .begin_open()
    }

    fn active_generations(&self) -> Vec<(HarnessId, u64)> {
        self.runtimes
            .iter()
            .filter(|(_, state)| state.command_sender.is_some())
            .map(|(harness_id, state)| (harness_id.clone(), state.generation))
            .collect()
    }

    fn has_active_commands(&self) -> bool {
        self.runtimes
            .values()
            .any(|state| state.command_sender.is_some())
    }
}

impl RuntimeEntryState {
    fn snapshot(&self, harness_id: HarnessId, metadata: RuntimeDriverMetadata) -> RuntimeSnapshot {
        RuntimeSnapshot {
            harness_id,
            metadata,
            phase: self.phase,
            generation: self.generation,
            cleanup_complete: self.cleanup_complete,
            failure_code: self.failure_code,
        }
    }

    fn blocks_another_runtime_launch(&self) -> bool {
        self.phase != RuntimePhase::Inactive
            && !(self.phase == RuntimePhase::Failed && self.cleanup_complete)
    }

    fn begin_open(&mut self) -> Result<OpenDirective, &'static str> {
        match self.phase {
            RuntimePhase::Inactive => self.begin_generation(),
            RuntimePhase::Failed if self.cleanup_complete => self.begin_generation(),
            RuntimePhase::Ready => Ok(OpenDirective::Focus {
                generation: self.generation,
            }),
            RuntimePhase::Starting | RuntimePhase::Stopping | RuntimePhase::Failed => {
                Ok(OpenDirective::Reuse)
            }
        }
    }

    fn begin_generation(&mut self) -> Result<OpenDirective, &'static str> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("harness.runtime-generation-exhausted")?;
        let (command_sender, commands) = mpsc::channel();
        self.phase = RuntimePhase::Starting;
        self.generation = generation;
        self.cleanup_complete = false;
        self.command_sender = Some(command_sender);
        self.failure_code = None;
        self.shutdown_requested = false;
        Ok(OpenDirective::Launch {
            generation,
            commands,
        })
    }

    fn begin_stop(&mut self, generation: u64) -> Option<mpsc::Sender<RuntimeControl>> {
        if self.generation != generation {
            return None;
        }
        match self.phase {
            RuntimePhase::Starting | RuntimePhase::Ready => {
                self.phase = RuntimePhase::Stopping;
                self.command_sender.clone()
            }
            RuntimePhase::Stopping => self.command_sender.clone(),
            RuntimePhase::Inactive | RuntimePhase::Failed => None,
        }
    }

    fn begin_control(
        &mut self,
        generation: u64,
        control: RuntimeControl,
    ) -> Option<mpsc::Sender<RuntimeControl>> {
        if self.generation != generation || self.shutdown_requested {
            return None;
        }
        if matches!(control, RuntimeControl::Shutdown) {
            self.shutdown_requested = true;
        }
        self.begin_stop(generation)
    }

    fn complete_generation(
        &mut self,
        generation: u64,
        phase: RuntimePhase,
        cleanup_complete: bool,
        failure_code: Option<&'static str>,
    ) -> bool {
        if self.generation != generation {
            return false;
        }
        self.phase = phase;
        self.cleanup_complete = cleanup_complete;
        self.command_sender = None;
        self.failure_code = failure_code;
        self.shutdown_requested = false;
        true
    }
}

impl RuntimeController {
    pub(crate) fn new(
        runtime_drivers: Vec<Arc<dyn HarnessRuntimeDriver>>,
        resolver: Arc<dyn InterfaceResolver>,
    ) -> Self {
        let mut drivers = BTreeMap::new();
        for driver in runtime_drivers {
            let harness_id = driver.harness_id().clone();
            assert!(
                drivers.insert(harness_id, driver).is_none(),
                "duplicate harness runtime driver"
            );
        }
        let state = ControllerState::new(drivers.keys().cloned());
        Self {
            shared: Arc::new(ControllerShared {
                state: Mutex::new(state),
                changed: Condvar::new(),
            }),
            drivers: Arc::new(drivers),
            resolver,
        }
    }

    pub(crate) fn snapshot(&self, harness_id: &HarnessId) -> Option<RuntimeSnapshot> {
        let driver = self.drivers.get(harness_id)?;
        self.lock_state()
            .runtimes
            .get(harness_id)
            .map(|state| state.snapshot(harness_id.clone(), driver.metadata()))
    }

    pub(crate) fn presentation_name(&self, harness_id: &HarnessId) -> Option<&str> {
        self.drivers
            .get(harness_id)
            .map(|driver| driver.presentation_name())
    }

    pub(crate) fn open_harness(
        &self,
        app: &tauri::AppHandle,
        harness_id: &str,
    ) -> Result<OpenHarnessResultDto, RuntimeFailureDto> {
        let harness_id = HarnessId::new(harness_id);
        let Some(driver) = self.drivers.get(&harness_id).cloned() else {
            return Err(RuntimeFailureDto::new(
                &harness_id,
                "harness.runtime-not-available",
            ));
        };

        let directive = {
            let mut state = self.lock_state();
            state
                .begin_open(&harness_id)
                .map_err(|code| RuntimeFailureDto::new(&harness_id, code))?
        };

        match directive {
            OpenDirective::Launch {
                generation,
                commands,
            } => {
                self.emit_snapshot(app, &harness_id);
                let worker_controller = self.clone();
                let worker_app = app.clone();
                let worker_harness_id = harness_id.clone();
                let thread_name = format!("{}-runtime-{generation}", harness_id.as_str());
                let spawn = thread::Builder::new().name(thread_name).spawn(move || {
                    let reporter = RuntimeGenerationReporter::new(
                        Arc::new(worker_controller.clone()),
                        worker_harness_id.clone(),
                        generation,
                    );
                    let context = RuntimeRunContext::new(
                        worker_app.clone(),
                        OwnedRuntimeIdentity::new(worker_harness_id.clone(), generation),
                        commands,
                        reporter,
                    );
                    let completion = driver.run_generation(context);
                    worker_controller.finish_generation(
                        &worker_app,
                        &worker_harness_id,
                        generation,
                        completion,
                    );
                });
                if spawn.is_err() {
                    self.finish_generation(
                        app,
                        &harness_id,
                        generation,
                        RuntimeCompletion::failed(
                            RuntimeFailure::new(
                                harness_id.clone(),
                                "harness.runtime-worker-unavailable",
                            ),
                            true,
                        ),
                    );
                }
            }
            OpenDirective::Focus { generation } => {
                if driver.focus_presentation(app).is_err()
                    && driver.metadata().presentation_close
                        == PresentationCloseSemantics::StopRuntime
                {
                    self.request_stop(app, &harness_id, generation, RuntimeControl::Stop);
                }
            }
            OpenDirective::Reuse => {}
        }

        Ok(self.open_result(&harness_id))
    }

    pub(crate) fn shutdown_for_app_exit(&self, app: &tauri::AppHandle) -> bool {
        let active = self.lock_state().active_generations();
        if active.is_empty() {
            return true;
        }
        for (harness_id, generation) in active {
            self.request_stop(app, &harness_id, generation, RuntimeControl::Shutdown);
        }

        let deadline = Instant::now() + APP_EXIT_WAIT;
        let mut state = self.lock_state();
        while state.has_active_commands() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, timeout) = self
                .shared
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
            if timeout.timed_out() && state.has_active_commands() {
                return false;
            }
        }
        true
    }

    fn request_stop(
        &self,
        app: &tauri::AppHandle,
        harness_id: &HarnessId,
        generation: u64,
        control: RuntimeControl,
    ) {
        let sender = {
            let mut state = self.lock_state();
            state
                .runtimes
                .get_mut(harness_id)
                .and_then(|runtime| runtime.begin_control(generation, control))
        };
        if let Some(sender) = sender {
            self.emit_snapshot(app, harness_id);
            let _ = sender.send(control);
        }
    }

    fn publish_ready_for_generation(
        &self,
        app: &tauri::AppHandle,
        harness_id: &HarnessId,
        generation: u64,
    ) {
        let changed = {
            let mut state = self.lock_state();
            let Some(runtime) = state.runtimes.get_mut(harness_id) else {
                return;
            };
            if runtime.generation != generation || runtime.phase != RuntimePhase::Starting {
                false
            } else {
                runtime.phase = RuntimePhase::Ready;
                true
            }
        };
        if changed {
            self.emit_snapshot(app, harness_id);
        }
    }

    fn finish_generation(
        &self,
        app: &tauri::AppHandle,
        harness_id: &HarnessId,
        generation: u64,
        completion: RuntimeCompletion,
    ) {
        debug_assert!(completion
            .failure()
            .is_none_or(|failure| failure.harness_id() == harness_id));
        let failure_code = completion.failure().map(RuntimeFailure::code);
        let changed = {
            let mut state = self.lock_state();
            let changed = state.runtimes.get_mut(harness_id).is_some_and(|runtime| {
                runtime.complete_generation(
                    generation,
                    completion.phase(),
                    completion.cleanup_complete(),
                    failure_code,
                )
            });
            if changed {
                self.shared.changed.notify_all();
            }
            changed
        };
        if changed {
            self.emit_snapshot(app, harness_id);
        }
    }

    fn open_result(&self, harness_id: &HarnessId) -> OpenHarnessResultDto {
        let snapshot = self
            .snapshot(harness_id)
            .expect("an open result requires a registered runtime driver");
        OpenHarnessResultDto {
            harness_id: harness_id.as_str().to_string(),
            phase: snapshot.phase(),
            can_open: snapshot.can_open(),
            surface: self.resolve_surface(&snapshot),
        }
    }

    fn emit_snapshot(&self, app: &tauri::AppHandle, harness_id: &HarnessId) {
        let Some(snapshot) = self.snapshot(harness_id) else {
            return;
        };
        let payload = self.event_payload(&snapshot);
        let _ = app.emit_to(crate::MAIN_WINDOW_LABEL, RUNTIME_CHANGED_EVENT, payload);
    }

    fn event_payload(&self, snapshot: &RuntimeSnapshot) -> RuntimeChangedEventDto {
        RuntimeChangedEventDto {
            harness_id: snapshot.harness_id().as_str().to_string(),
            phase: snapshot.phase(),
            can_open: snapshot.can_open(),
            surface: self.resolve_surface(snapshot),
        }
    }

    pub(crate) fn surface(&self) -> HostSurfaceState {
        let snapshots = self
            .drivers
            .keys()
            .filter_map(|harness_id| self.snapshot(harness_id))
            .collect::<Vec<_>>();
        let selected = snapshots
            .iter()
            .find(|snapshot| snapshot.phase() == RuntimePhase::Ready)
            .or_else(|| {
                snapshots.iter().find(|snapshot| {
                    matches!(
                        snapshot.phase(),
                        RuntimePhase::Starting | RuntimePhase::Stopping
                    )
                })
            })
            .or_else(|| {
                snapshots
                    .iter()
                    .find(|snapshot| snapshot.phase() == RuntimePhase::Failed)
            });
        selected.map_or_else(
            || self.resolver.resolve(InterfaceFacts::NoHarnessActive),
            |snapshot| self.resolve_surface(snapshot),
        )
    }

    pub(crate) fn resolve_surface(&self, snapshot: &RuntimeSnapshot) -> HostSurfaceState {
        let driver = self
            .drivers
            .get(snapshot.harness_id())
            .expect("a runtime snapshot requires its registered driver");
        let harness_name = driver.presentation_name().to_string();
        let facts = match snapshot.phase() {
            RuntimePhase::Inactive => InterfaceFacts::NoHarnessActive,
            RuntimePhase::Starting | RuntimePhase::Stopping => {
                InterfaceFacts::Loading { harness_name }
            }
            RuntimePhase::Ready => InterfaceFacts::OfficialInterfaceAvailable {
                harness_name,
                presentation: snapshot.metadata().presentation,
            },
            RuntimePhase::Failed => InterfaceFacts::Error(format!(
                "The official {harness_name} interface could not be opened."
            )),
        };
        self.resolver.resolve(facts)
    }

    fn presentation_close_control(&self, harness_id: &HarnessId) -> Option<RuntimeControl> {
        match self.drivers.get(harness_id)?.metadata().presentation_close {
            PresentationCloseSemantics::StopRuntime => Some(RuntimeControl::Stop),
            PresentationCloseSemantics::RuntimeContinues => None,
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, ControllerState> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    pub(crate) fn set_snapshot_for_test(
        &self,
        harness_id: &HarnessId,
        phase: RuntimePhase,
        cleanup_complete: bool,
        failure_code: Option<&'static str>,
    ) {
        let mut state = self.lock_state();
        let runtime = state
            .runtimes
            .get_mut(harness_id)
            .expect("test runtime driver must be registered");
        runtime.phase = phase;
        runtime.cleanup_complete = cleanup_complete;
        runtime.failure_code = failure_code;
    }
}

impl RuntimeEventSink for RuntimeController {
    fn publish_ready(&self, app: &tauri::AppHandle, harness_id: &HarnessId, generation: u64) {
        self.publish_ready_for_generation(app, harness_id, generation);
    }

    fn presentation_closed(&self, app: &tauri::AppHandle, harness_id: &HarnessId, generation: u64) {
        if let Some(control) = self.presentation_close_control(harness_id) {
            self.request_stop(app, harness_id, generation, control);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::adapter::ExecutionTopology;
    use crate::interface::DefaultInterfaceResolver;
    use crate::runtime::driver::{
        RuntimeAuthenticationClass, RuntimeOwnershipClass, RuntimePresentationClass,
        RuntimeReadinessClass,
    };

    struct FakeDriver {
        harness_id: HarnessId,
        presentation_name: String,
        metadata: RuntimeDriverMetadata,
    }

    impl FakeDriver {
        fn deepseek() -> Self {
            Self {
                harness_id: HarnessId::new("deepseek"),
                presentation_name: "DeepSeek".to_string(),
                metadata: RuntimeDriverMetadata {
                    topology: ExecutionTopology::NativeWindows,
                    ownership: RuntimeOwnershipClass::WindowsJob,
                    readiness: RuntimeReadinessClass::StdoutLaunchToken,
                    authentication: RuntimeAuthenticationClass::LaunchTokenSessionCookie,
                    presentation: RuntimePresentationClass::OwnedIncognitoWebview,
                    presentation_close: PresentationCloseSemantics::StopRuntime,
                },
            }
        }

        fn opencode_shape() -> Self {
            Self {
                harness_id: HarnessId::new("opencode"),
                presentation_name: "OpenCode".to_string(),
                metadata: RuntimeDriverMetadata {
                    topology: ExecutionTopology::WslNative,
                    ownership: RuntimeOwnershipClass::SystemdUserServiceCgroup,
                    readiness: RuntimeReadinessClass::AuthenticatedHttp,
                    authentication: RuntimeAuthenticationClass::BasicAuthentication,
                    presentation: RuntimePresentationClass::PersistentExternalBrowser,
                    presentation_close: PresentationCloseSemantics::RuntimeContinues,
                },
            }
        }
    }

    impl HarnessRuntimeDriver for FakeDriver {
        fn harness_id(&self) -> &HarnessId {
            &self.harness_id
        }

        fn presentation_name(&self) -> &str {
            &self.presentation_name
        }

        fn metadata(&self) -> RuntimeDriverMetadata {
            self.metadata
        }

        fn run_generation(&self, _context: RuntimeRunContext) -> RuntimeCompletion {
            panic!("the contract test does not launch a runtime")
        }

        fn focus_presentation(&self, _app: &tauri::AppHandle) -> Result<(), RuntimeFailure> {
            Ok(())
        }
    }

    fn controller() -> RuntimeController {
        RuntimeController::new(
            vec![Arc::new(FakeDriver::deepseek())],
            Arc::new(DefaultInterfaceResolver),
        )
    }

    fn controller_with_two_drivers() -> RuntimeController {
        RuntimeController::new(
            vec![
                Arc::new(FakeDriver::deepseek()),
                Arc::new(FakeDriver::opencode_shape()),
            ],
            Arc::new(DefaultInterfaceResolver),
        )
    }

    fn entry() -> RuntimeEntryState {
        RuntimeEntryState::default()
    }

    #[test]
    fn phase_4c_inactive_open_creates_exactly_one_generation() {
        let mut state = entry();
        assert!(matches!(
            state.begin_open().unwrap(),
            OpenDirective::Launch { generation: 1, .. }
        ));
        assert_eq!(state.generation, 1);
        assert_eq!(state.phase, RuntimePhase::Starting);
    }

    #[test]
    fn phase_4c_concurrent_open_requests_create_one_launch_directive() {
        let state = Arc::new(Mutex::new(entry()));
        let mut threads = Vec::new();
        for _ in 0..16 {
            let state = Arc::clone(&state);
            threads.push(thread::spawn(move || {
                let mut state = state.lock().unwrap();
                matches!(state.begin_open().unwrap(), OpenDirective::Launch { .. })
            }));
        }
        let launches = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .filter(|launched| *launched)
            .count();
        assert_eq!(launches, 1);
        assert_eq!(state.lock().unwrap().generation, 1);
    }

    #[test]
    fn phase_4c_starting_and_ready_open_never_relaunch() {
        let mut state = entry();
        let _ = state.begin_open().unwrap();
        assert!(matches!(state.begin_open().unwrap(), OpenDirective::Reuse));
        state.phase = RuntimePhase::Ready;
        assert!(matches!(
            state.begin_open().unwrap(),
            OpenDirective::Focus { generation: 1 }
        ));
        assert_eq!(state.generation, 1);
    }

    #[test]
    fn phase_4c_presentation_close_transitions_only_the_owned_generation_to_stopping() {
        let mut state = entry();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };
        state.phase = RuntimePhase::Ready;
        assert!(state.begin_control(2, RuntimeControl::Stop).is_none());
        assert_eq!(state.phase, RuntimePhase::Ready);
        let sender = state
            .begin_control(1, RuntimeControl::Stop)
            .expect("owned generation should stop");
        sender.send(RuntimeControl::Stop).unwrap();
        assert_eq!(commands.recv().unwrap(), RuntimeControl::Stop);
        assert_eq!(state.phase, RuntimePhase::Stopping);
        assert!(!state.shutdown_requested);
    }

    #[test]
    fn phase_4c_reopen_after_normal_close_creates_one_new_generation() {
        let mut state = entry();
        let _ = state.begin_open().unwrap();
        state.phase = RuntimePhase::Ready;
        let _ = state.begin_stop(1).unwrap();
        state.phase = RuntimePhase::Inactive;
        state.cleanup_complete = true;
        state.command_sender = None;
        assert!(matches!(
            state.begin_open().unwrap(),
            OpenDirective::Launch { generation: 2, .. }
        ));
        assert_eq!(state.generation, 2);
    }

    #[test]
    fn phase_4c_failed_runtime_retries_only_after_cleanup_and_explicit_open() {
        let mut state = RuntimeEntryState {
            phase: RuntimePhase::Failed,
            generation: 1,
            cleanup_complete: false,
            command_sender: None,
            failure_code: Some("deepseek.test-failure"),
            shutdown_requested: false,
        };
        assert!(matches!(state.begin_open().unwrap(), OpenDirective::Reuse));
        assert_eq!(state.generation, 1);
        state.cleanup_complete = true;
        assert_eq!(state.generation, 1, "cleanup must not retry automatically");
        assert!(matches!(
            state.begin_open().unwrap(),
            OpenDirective::Launch { generation: 2, .. }
        ));
    }

    #[test]
    fn phase_4c_shutdown_control_uses_only_the_active_owned_generation() {
        let mut state = entry();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };
        assert!(state.begin_control(2, RuntimeControl::Shutdown).is_none());
        let sender = state
            .begin_control(1, RuntimeControl::Shutdown)
            .expect("owned generation should stop");
        sender.send(RuntimeControl::Shutdown).unwrap();
        assert_eq!(commands.recv().unwrap(), RuntimeControl::Shutdown);
        assert_eq!(state.phase, RuntimePhase::Stopping);
        assert!(state.shutdown_requested);
        let application_lifecycle = include_str!("../lib.rs");
        assert!(application_lifecycle.contains("shutdown_for_app_exit"));
        assert!(application_lifecycle.contains("WindowEvent::CloseRequested"));
        assert!(application_lifecycle.contains("RunEvent::ExitRequested"));
    }

    #[test]
    fn correction_2_ready_shutdown_closes_presentation_and_completes_inactive() {
        let mut state = entry();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };
        state.phase = RuntimePhase::Ready;
        let sender = state
            .begin_control(1, RuntimeControl::Shutdown)
            .expect("ready runtime should accept shutdown");
        sender.send(RuntimeControl::Shutdown).unwrap();
        assert_eq!(commands.recv().unwrap(), RuntimeControl::Shutdown);
        assert_eq!(state.phase, RuntimePhase::Stopping);
        assert!(state.complete_generation(1, RuntimePhase::Inactive, true, None));
        assert_eq!(state.phase, RuntimePhase::Inactive);
        assert!(state.cleanup_complete);
        assert!(state.command_sender.is_none());
    }

    #[test]
    fn correction_2_starting_shutdown_is_observed_before_launch_and_cleans_up() {
        let mut state = entry();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };
        let sender = state
            .begin_control(1, RuntimeControl::Shutdown)
            .expect("starting runtime should accept shutdown");
        sender.send(RuntimeControl::Shutdown).unwrap();
        assert_eq!(commands.recv().unwrap(), RuntimeControl::Shutdown);
        assert_eq!(state.phase, RuntimePhase::Stopping);
        assert!(state.complete_generation(1, RuntimePhase::Inactive, true, None));
        assert_eq!(state.phase, RuntimePhase::Inactive);
        assert!(state.cleanup_complete);
    }

    #[test]
    fn correction_2_repeated_shutdown_requests_are_idempotent() {
        let mut state = entry();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };
        state.phase = RuntimePhase::Ready;
        let sender = state
            .begin_control(1, RuntimeControl::Shutdown)
            .expect("first shutdown should be sent");
        assert!(state.begin_control(1, RuntimeControl::Shutdown).is_none());
        assert!(state.begin_control(1, RuntimeControl::Stop).is_none());
        sender.send(RuntimeControl::Shutdown).unwrap();
        assert_eq!(commands.recv().unwrap(), RuntimeControl::Shutdown);
        assert!(matches!(
            commands.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert_eq!(state.phase, RuntimePhase::Stopping);
    }

    #[test]
    fn correction_2_stopping_runtime_is_upgraded_once_to_application_shutdown() {
        let mut state = entry();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };
        state.phase = RuntimePhase::Ready;
        let stop = state
            .begin_control(1, RuntimeControl::Stop)
            .expect("presentation stop should start cleanup");
        stop.send(RuntimeControl::Stop).unwrap();
        let shutdown = state
            .begin_control(1, RuntimeControl::Shutdown)
            .expect("main close should upgrade the existing cleanup");
        shutdown.send(RuntimeControl::Shutdown).unwrap();
        assert!(state.begin_control(1, RuntimeControl::Shutdown).is_none());
        assert_eq!(commands.recv().unwrap(), RuntimeControl::Stop);
        assert_eq!(commands.recv().unwrap(), RuntimeControl::Shutdown);
        assert!(state.shutdown_requested);
        assert_eq!(state.phase, RuntimePhase::Stopping);
    }

    #[test]
    fn correction_2_deepseek_controls_still_close_the_presentation_intentionally() {
        let worker = include_str!("deepseek_driver.rs");
        assert!(worker.contains("PresentationHandle::close_intentionally"));
        assert!(worker.contains("runtime.stop()"));
    }

    #[test]
    fn oc3_snapshot_identity_metadata_and_state_are_keyed_per_driver() {
        let controller = controller_with_two_drivers();
        let deepseek = controller.snapshot(&HarnessId::new("deepseek")).unwrap();
        let opencode = controller.snapshot(&HarnessId::new("opencode")).unwrap();
        assert_eq!(deepseek.harness_id().as_str(), "deepseek");
        assert_eq!(
            deepseek.metadata().ownership,
            RuntimeOwnershipClass::WindowsJob
        );
        assert_eq!(opencode.harness_id().as_str(), "opencode");
        assert_eq!(opencode.metadata().topology, ExecutionTopology::WslNative);
        assert_eq!(
            opencode.metadata().ownership,
            RuntimeOwnershipClass::SystemdUserServiceCgroup
        );
        assert_eq!(
            opencode.metadata().readiness,
            RuntimeReadinessClass::AuthenticatedHttp
        );
        assert_eq!(
            opencode.metadata().authentication,
            RuntimeAuthenticationClass::BasicAuthentication
        );
        assert_eq!(
            opencode.metadata().presentation_close,
            PresentationCloseSemantics::RuntimeContinues
        );
        controller.set_snapshot_for_test(
            &HarnessId::new("deepseek"),
            RuntimePhase::Starting,
            false,
            None,
        );
        assert_eq!(
            controller
                .snapshot(&HarnessId::new("opencode"))
                .unwrap()
                .phase(),
            RuntimePhase::Inactive
        );
    }

    #[test]
    fn oc3_presentation_close_semantics_vary_without_controller_changes() {
        let controller = controller_with_two_drivers();
        assert_eq!(
            controller.presentation_close_control(&HarnessId::new("deepseek")),
            Some(RuntimeControl::Stop)
        );
        assert_eq!(
            controller.presentation_close_control(&HarnessId::new("opencode")),
            None
        );
    }

    #[test]
    fn oc3_controller_does_not_expand_into_simultaneous_runtime_execution() {
        let mut state =
            ControllerState::new([HarnessId::new("deepseek"), HarnessId::new("opencode")]);
        assert!(matches!(
            state.begin_open(&HarnessId::new("deepseek")).unwrap(),
            OpenDirective::Launch { .. }
        ));
        assert_eq!(
            state.begin_open(&HarnessId::new("opencode")).err(),
            Some("harness.runtime-busy")
        );
    }

    #[test]
    fn oc3_generic_failure_dto_retains_harness_identity() {
        let failure = RuntimeFailureDto::from(RuntimeFailure::new(
            HarnessId::new("opencode"),
            "opencode.test-failure",
        ));
        let json = serde_json::to_value(failure).unwrap();
        assert_eq!(json["harnessId"], "opencode");
        assert_eq!(json["code"], "opencode.test-failure");
    }

    #[test]
    fn oc3_generic_results_and_events_retain_harness_identity() {
        let controller = controller_with_two_drivers();
        let harness_id = HarnessId::new("opencode");
        let result = controller.open_result(&harness_id);
        let snapshot = controller.snapshot(&harness_id).unwrap();
        let event = controller.event_payload(&snapshot);
        assert_eq!(result.harness_id, "opencode");
        assert_eq!(event.harness_id, "opencode");
    }

    #[test]
    fn phase_4c_public_results_and_events_are_credential_free() {
        let controller = controller();
        let snapshot = controller.snapshot(&HarnessId::new("deepseek")).unwrap();
        assert_eq!(snapshot.harness_id().as_str(), "deepseek");
        let result = OpenHarnessResultDto {
            harness_id: snapshot.harness_id().as_str().to_string(),
            phase: snapshot.phase(),
            can_open: snapshot.can_open(),
            surface: controller.resolve_surface(&snapshot),
        };
        let event = RuntimeChangedEventDto {
            harness_id: snapshot.harness_id().as_str().to_string(),
            phase: snapshot.phase(),
            can_open: snapshot.can_open(),
            surface: controller.resolve_surface(&snapshot),
        };
        let serialized = format!(
            "{}{}",
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&event).unwrap()
        );
        assert!(serialized.contains("\"harnessId\":\"deepseek\""));
        for forbidden in [
            "token",
            "authenticatedUrl",
            "?token=",
            "127.0.0.1",
            "processCommandLine",
            "environment",
            "pid",
            "generation",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "unexpected value: {forbidden}"
            );
        }
    }
}
