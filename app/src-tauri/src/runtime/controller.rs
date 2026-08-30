use crate::harness::adapter::{DetectionContext, HarnessId};
use crate::harness::deepseek::DEEPSEEK_ADAPTER_ID;
use crate::harness::registry::HarnessRegistry;
use crate::interface::{HostSurfaceState, InterfaceFacts, InterfaceResolver};
use crate::runtime::domain::RuntimePhase;
use crate::runtime::presentation::{
    focus_existing_deepseek_interface, present_official_deepseek_interface, PresentationHandle,
};
use crate::runtime::windows::deepseek::{
    prepare_deepseek_launch, DeepSeekOwnedRuntime, DeepSeekReadinessUpdate,
};
use serde::Serialize;
use std::fs::File;
use std::io::Read;
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
use tauri::Emitter;

pub(crate) const RUNTIME_CHANGED_EVENT: &str = "harness-runtime-changed";
const OUTPUT_CHANNEL_CAPACITY: usize = 64;
const OUTPUT_CHUNK_BYTES: usize = 2_048;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(50);
const APP_EXIT_WAIT: Duration = Duration::from_secs(12);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeSnapshot {
    phase: RuntimePhase,
    cleanup_complete: bool,
    failure_code: Option<&'static str>,
}

impl RuntimeSnapshot {
    pub(crate) fn phase(self) -> RuntimePhase {
        self.phase
    }

    pub(crate) fn can_open(self) -> bool {
        self.phase == RuntimePhase::Inactive
            || (self.phase == RuntimePhase::Failed && self.cleanup_complete)
            || self.phase == RuntimePhase::Ready
    }

    pub(crate) fn failure_code(self) -> Option<&'static str> {
        self.failure_code
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenHarnessResultDto {
    phase: RuntimePhase,
    can_open: bool,
    surface: HostSurfaceState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeChangedEventDto {
    harness_id: &'static str,
    phase: RuntimePhase,
    can_open: bool,
    surface: HostSurfaceState,
}

#[derive(Clone)]
pub(crate) struct RuntimeController {
    shared: Arc<ControllerShared>,
    registry: Arc<HarnessRegistry>,
    detection_context: DetectionContext,
    resolver: Arc<dyn InterfaceResolver>,
}

struct ControllerShared {
    state: Mutex<ControllerState>,
    changed: Condvar,
}

struct ControllerState {
    phase: RuntimePhase,
    generation: u64,
    cleanup_complete: bool,
    command_sender: Option<mpsc::Sender<Control>>,
    failure_code: Option<&'static str>,
    shutdown_requested: bool,
}

impl Default for ControllerState {
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
        commands: mpsc::Receiver<Control>,
    },
    Focus {
        generation: u64,
    },
    Reuse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Control {
    Stop,
    Shutdown,
}

enum OutputUpdate {
    Stdout(Vec<u8>),
    Stderr(String),
    ReadFailed,
    Closed,
}

impl ControllerState {
    fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            phase: self.phase,
            cleanup_complete: self.cleanup_complete,
            failure_code: self.failure_code,
        }
    }

    fn begin_open(&mut self) -> Result<OpenDirective, &'static str> {
        match self.phase {
            RuntimePhase::Inactive => {
                let generation = self
                    .generation
                    .checked_add(1)
                    .ok_or("deepseek.runtime-generation-exhausted")?;
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
            RuntimePhase::Failed if self.cleanup_complete => {
                let generation = self
                    .generation
                    .checked_add(1)
                    .ok_or("deepseek.runtime-generation-exhausted")?;
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
            RuntimePhase::Ready => Ok(OpenDirective::Focus {
                generation: self.generation,
            }),
            RuntimePhase::Starting | RuntimePhase::Stopping | RuntimePhase::Failed => {
                Ok(OpenDirective::Reuse)
            }
        }
    }

    fn begin_stop(&mut self, generation: u64) -> Option<mpsc::Sender<Control>> {
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
        control: Control,
    ) -> Option<mpsc::Sender<Control>> {
        if self.generation != generation || self.shutdown_requested {
            return None;
        }
        if matches!(control, Control::Shutdown) {
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

impl Control {
    fn closes_presentation_intentionally(self) -> bool {
        matches!(self, Self::Stop | Self::Shutdown)
    }
}

impl RuntimeController {
    pub(crate) fn new(
        registry: Arc<HarnessRegistry>,
        detection_context: DetectionContext,
        resolver: Arc<dyn InterfaceResolver>,
    ) -> Self {
        Self {
            shared: Arc::new(ControllerShared {
                state: Mutex::new(ControllerState::default()),
                changed: Condvar::new(),
            }),
            registry,
            detection_context,
            resolver,
        }
    }

    pub(crate) fn snapshot(&self, harness_id: &HarnessId) -> Option<RuntimeSnapshot> {
        if harness_id.as_str() != DEEPSEEK_ADAPTER_ID {
            return None;
        }
        Some(self.lock_state().snapshot())
    }

    pub(crate) fn open_harness(
        &self,
        app: &tauri::AppHandle,
        harness_id: &str,
    ) -> Result<OpenHarnessResultDto, &'static str> {
        if harness_id != DEEPSEEK_ADAPTER_ID
            || self.registry.get(&HarnessId::new(harness_id)).is_none()
        {
            return Err("harness.not-supported");
        }

        let directive = {
            let mut state = self.lock_state();
            state.begin_open()?
        };

        match directive {
            OpenDirective::Launch {
                generation,
                commands,
            } => {
                self.emit_snapshot(app);
                let worker_controller = self.clone();
                let worker_app = app.clone();
                let spawn = thread::Builder::new()
                    .name(format!("deepseek-runtime-{generation}"))
                    .spawn(move || {
                        worker_controller.run_generation(worker_app, generation, commands)
                    });
                if spawn.is_err() {
                    self.finish_generation(
                        app,
                        generation,
                        RuntimePhase::Failed,
                        true,
                        Some("deepseek.runtime-worker-unavailable"),
                    );
                }
            }
            OpenDirective::Focus { generation } => {
                if focus_existing_deepseek_interface(app).is_err() {
                    self.request_stop(app, generation, Control::Stop);
                }
            }
            OpenDirective::Reuse => {}
        }

        Ok(self.open_result())
    }

    pub(crate) fn request_presentation_close(&self, app: &tauri::AppHandle, generation: u64) {
        self.request_stop(app, generation, Control::Stop);
    }

    pub(crate) fn shutdown_for_app_exit(&self, app: &tauri::AppHandle) -> bool {
        let generation = {
            let state = self.lock_state();
            if state.command_sender.is_none() {
                return true;
            }
            state.generation
        };
        self.request_stop(app, generation, Control::Shutdown);

        let deadline = Instant::now() + APP_EXIT_WAIT;
        let mut state = self.lock_state();
        while state.command_sender.is_some() {
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
            if timeout.timed_out() && state.command_sender.is_some() {
                return false;
            }
        }
        true
    }

    fn run_generation(
        &self,
        app: tauri::AppHandle,
        generation: u64,
        commands: mpsc::Receiver<Control>,
    ) {
        if let Some(control) = pending_control(&commands) {
            self.finish_cancelled_launch(&app, generation, control);
            return;
        }

        let plan = match prepare_deepseek_launch(&self.registry, &self.detection_context) {
            Ok(plan) => plan,
            Err(error) => {
                self.finish_generation(
                    &app,
                    generation,
                    RuntimePhase::Failed,
                    true,
                    Some(error.code()),
                );
                return;
            }
        };

        if let Some(control) = pending_control(&commands) {
            self.finish_cancelled_launch(&app, generation, control);
            return;
        }

        let mut runtime = match DeepSeekOwnedRuntime::launch(plan, generation) {
            Ok(runtime) => runtime,
            Err(error) => {
                self.finish_generation(
                    &app,
                    generation,
                    RuntimePhase::Failed,
                    true,
                    Some(error.code()),
                );
                return;
            }
        };
        let output = start_output_pumps(&mut runtime);
        let mut presentation: Option<PresentationHandle> = None;

        loop {
            if let Some(control) = pending_control(&commands) {
                let close_result = if control.closes_presentation_intentionally() {
                    presentation
                        .take()
                        .map(PresentationHandle::close_intentionally)
                        .transpose()
                        .map(|_| ())
                } else {
                    Ok(())
                };
                let stopped = runtime.stop_owned();
                let (phase, cleanup_complete, failure_code) = match (stopped, close_result) {
                    (Ok(()), Ok(())) => (RuntimePhase::Inactive, true, None),
                    (Err(error), _) => (RuntimePhase::Failed, false, Some(error.code())),
                    (Ok(()), Err(error)) => (RuntimePhase::Failed, true, Some(error.code())),
                };
                self.finish_generation(&app, generation, phase, cleanup_complete, failure_code);
                return;
            }

            match output.recv_timeout(WORKER_POLL_INTERVAL) {
                Ok(OutputUpdate::Stdout(chunk)) => match runtime.ingest_stdout(&chunk) {
                    Ok(DeepSeekReadinessUpdate::Pending) => {}
                    Ok(DeepSeekReadinessUpdate::Ready { .. }) => {
                        let Some(target) = runtime.take_ready_target() else {
                            let cleaned = runtime
                                .fail_owned_runtime(
                                    "deepseek.ready-target-unavailable",
                                    "The private DeepSeek readiness target was unavailable.",
                                )
                                .is_ok();
                            self.finish_generation(
                                &app,
                                generation,
                                RuntimePhase::Failed,
                                cleaned,
                                Some("deepseek.ready-target-unavailable"),
                            );
                            return;
                        };
                        match present_official_deepseek_interface(
                            &app,
                            target,
                            generation,
                            self.clone(),
                        ) {
                            Ok(window) => {
                                presentation = Some(window);
                                self.publish_ready(&app, generation);
                            }
                            Err(error) => {
                                let code = error.code();
                                let cleaned = runtime
                                    .fail_owned_runtime(
                                        code,
                                        "The official DeepSeek interface could not be presented.",
                                    )
                                    .is_ok();
                                self.finish_generation(
                                    &app,
                                    generation,
                                    RuntimePhase::Failed,
                                    cleaned,
                                    Some(code),
                                );
                                return;
                            }
                        }
                    }
                    Ok(DeepSeekReadinessUpdate::Failed { code }) => {
                        close_invalid_presentation(presentation.take());
                        self.finish_generation(
                            &app,
                            generation,
                            RuntimePhase::Failed,
                            true,
                            Some(code),
                        );
                        return;
                    }
                    Err(error) => {
                        close_invalid_presentation(presentation.take());
                        self.finish_generation(
                            &app,
                            generation,
                            RuntimePhase::Failed,
                            false,
                            Some(error.code()),
                        );
                        return;
                    }
                },
                Ok(OutputUpdate::Stderr(chunk)) => runtime.record_sanitized_stderr(&chunk),
                Ok(OutputUpdate::ReadFailed) => {
                    let code = "deepseek.output-read-failed";
                    let cleaned = runtime
                        .fail_owned_runtime(
                            code,
                            "Owned DeepSeek output could not be observed safely.",
                        )
                        .is_ok();
                    close_invalid_presentation(presentation.take());
                    self.finish_generation(
                        &app,
                        generation,
                        RuntimePhase::Failed,
                        cleaned,
                        Some(code),
                    );
                    return;
                }
                Ok(OutputUpdate::Closed) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }

            match runtime.wait_for_root_exit(Duration::ZERO) {
                Ok(false) => {}
                Ok(true) => {
                    let code = if runtime.phase() == RuntimePhase::Ready {
                        "deepseek.runtime-exited-unexpectedly"
                    } else {
                        "deepseek.process-exited-before-readiness"
                    };
                    let cleaned = runtime.reconcile_unexpected_root_exit().is_ok();
                    close_invalid_presentation(presentation.take());
                    self.finish_generation(
                        &app,
                        generation,
                        RuntimePhase::Failed,
                        cleaned,
                        Some(code),
                    );
                    return;
                }
                Err(error) => {
                    let code = error.code();
                    let cleaned = runtime
                        .fail_owned_runtime(
                            code,
                            "The owned DeepSeek process could not be observed safely.",
                        )
                        .is_ok();
                    close_invalid_presentation(presentation.take());
                    self.finish_generation(
                        &app,
                        generation,
                        RuntimePhase::Failed,
                        cleaned,
                        Some(code),
                    );
                    return;
                }
            }
        }
    }

    fn finish_cancelled_launch(&self, app: &tauri::AppHandle, generation: u64, _control: Control) {
        self.finish_generation(app, generation, RuntimePhase::Inactive, true, None);
    }

    fn request_stop(&self, app: &tauri::AppHandle, generation: u64, control: Control) {
        let sender = {
            let mut state = self.lock_state();
            state.begin_control(generation, control)
        };
        if let Some(sender) = sender {
            self.emit_snapshot(app);
            let _ = sender.send(control);
        }
    }

    fn publish_ready(&self, app: &tauri::AppHandle, generation: u64) {
        let changed = {
            let mut state = self.lock_state();
            if state.generation != generation || state.phase != RuntimePhase::Starting {
                false
            } else {
                state.phase = RuntimePhase::Ready;
                true
            }
        };
        if changed {
            self.emit_snapshot(app);
        }
    }

    fn finish_generation(
        &self,
        app: &tauri::AppHandle,
        generation: u64,
        phase: RuntimePhase,
        cleanup_complete: bool,
        failure_code: Option<&'static str>,
    ) {
        let changed = {
            let mut state = self.lock_state();
            let changed =
                state.complete_generation(generation, phase, cleanup_complete, failure_code);
            if changed {
                self.shared.changed.notify_all();
            }
            changed
        };
        if changed {
            self.emit_snapshot(app);
        }
    }

    fn open_result(&self) -> OpenHarnessResultDto {
        let snapshot = self.lock_state().snapshot();
        OpenHarnessResultDto {
            phase: snapshot.phase(),
            can_open: snapshot.can_open(),
            surface: self.resolve_surface(snapshot),
        }
    }

    fn emit_snapshot(&self, app: &tauri::AppHandle) {
        let snapshot = self.lock_state().snapshot();
        let payload = RuntimeChangedEventDto {
            harness_id: DEEPSEEK_ADAPTER_ID,
            phase: snapshot.phase(),
            can_open: snapshot.can_open(),
            surface: self.resolve_surface(snapshot),
        };
        let _ = app.emit_to(crate::MAIN_WINDOW_LABEL, RUNTIME_CHANGED_EVENT, payload);
    }

    pub(crate) fn resolve_surface(&self, snapshot: RuntimeSnapshot) -> HostSurfaceState {
        let facts = match snapshot.phase() {
            RuntimePhase::Inactive => InterfaceFacts::NoHarnessActive,
            RuntimePhase::Starting | RuntimePhase::Stopping => InterfaceFacts::Loading,
            RuntimePhase::Ready => InterfaceFacts::OfficialInterfaceAvailable,
            RuntimePhase::Failed => InterfaceFacts::Error(
                "The official DeepSeek interface could not be opened.".to_string(),
            ),
        };
        self.resolver.resolve(facts)
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
        phase: RuntimePhase,
        cleanup_complete: bool,
        failure_code: Option<&'static str>,
    ) {
        let mut state = self.lock_state();
        state.phase = phase;
        state.cleanup_complete = cleanup_complete;
        state.failure_code = failure_code;
    }
}

fn pending_control(commands: &mpsc::Receiver<Control>) -> Option<Control> {
    match commands.try_recv() {
        Ok(control) => Some(control),
        Err(mpsc::TryRecvError::Empty) => None,
        Err(mpsc::TryRecvError::Disconnected) => Some(Control::Shutdown),
    }
}

fn start_output_pumps(runtime: &mut DeepSeekOwnedRuntime) -> mpsc::Receiver<OutputUpdate> {
    let (sender, receiver) = mpsc::sync_channel(OUTPUT_CHANNEL_CAPACITY);
    let mut pump_count = 0;
    if let Some(stdout) = runtime.take_stdout() {
        pump_count += 1;
        if !spawn_output_pump(stdout, sender.clone(), true) {
            let _ = sender.try_send(OutputUpdate::ReadFailed);
        }
    }
    if let Some(stderr) = runtime.take_stderr() {
        pump_count += 1;
        if !spawn_output_pump(stderr, sender.clone(), false) {
            let _ = sender.try_send(OutputUpdate::ReadFailed);
        }
    }
    if pump_count != 2 {
        let _ = sender.try_send(OutputUpdate::ReadFailed);
    }
    drop(sender);
    receiver
}

fn spawn_output_pump(
    mut source: File,
    sender: mpsc::SyncSender<OutputUpdate>,
    is_stdout: bool,
) -> bool {
    let stream = if is_stdout { "stdout" } else { "stderr" };
    thread::Builder::new()
        .name(format!("deepseek-{stream}-pump"))
        .spawn(move || {
            let mut chunk = vec![0u8; OUTPUT_CHUNK_BYTES];
            loop {
                match source.read(&mut chunk) {
                    Ok(0) => {
                        let _ = sender.send(OutputUpdate::Closed);
                        return;
                    }
                    Ok(length) => {
                        let update = if is_stdout {
                            OutputUpdate::Stdout(chunk[..length].to_vec())
                        } else {
                            OutputUpdate::Stderr(
                                String::from_utf8_lossy(&chunk[..length]).into_owned(),
                            )
                        };
                        if sender.send(update).is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        let _ = sender.send(OutputUpdate::ReadFailed);
                        return;
                    }
                }
            }
        })
        .is_ok()
}

fn close_invalid_presentation(presentation: Option<PresentationHandle>) {
    if let Some(presentation) = presentation {
        let _ = presentation.close_intentionally();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::deepseek::DeepSeekAdapter;
    use crate::interface::DefaultInterfaceResolver;

    fn controller() -> RuntimeController {
        let mut registry = HarnessRegistry::default();
        registry.register(Arc::new(DeepSeekAdapter::new())).unwrap();
        RuntimeController::new(
            Arc::new(registry),
            DetectionContext::default(),
            Arc::new(DefaultInterfaceResolver),
        )
    }

    #[test]
    fn phase_4c_inactive_open_creates_exactly_one_generation() {
        let mut state = ControllerState::default();

        assert!(matches!(
            state.begin_open().unwrap(),
            OpenDirective::Launch { generation: 1, .. }
        ));
        assert_eq!(state.generation, 1);
        assert_eq!(state.phase, RuntimePhase::Starting);
    }

    #[test]
    fn phase_4c_concurrent_open_requests_create_one_launch_directive() {
        let state = Arc::new(Mutex::new(ControllerState::default()));
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
        let mut state = ControllerState::default();
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
        let mut state = ControllerState::default();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };
        state.phase = RuntimePhase::Ready;

        assert!(state.begin_control(2, Control::Stop).is_none());
        assert_eq!(state.phase, RuntimePhase::Ready);
        let sender = state
            .begin_control(1, Control::Stop)
            .expect("owned generation should stop");
        sender.send(Control::Stop).unwrap();
        assert_eq!(commands.recv().unwrap(), Control::Stop);
        assert_eq!(state.phase, RuntimePhase::Stopping);
        assert!(!state.shutdown_requested);
    }

    #[test]
    fn phase_4c_reopen_after_normal_close_creates_one_new_generation() {
        let mut state = ControllerState::default();
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
        let mut state = ControllerState {
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
        let mut state = ControllerState::default();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };

        assert!(state.begin_control(2, Control::Shutdown).is_none());
        let sender = state
            .begin_control(1, Control::Shutdown)
            .expect("owned generation should stop");
        sender.send(Control::Shutdown).unwrap();
        assert!(matches!(commands.recv().unwrap(), Control::Shutdown));
        assert_eq!(state.phase, RuntimePhase::Stopping);
        assert!(state.shutdown_requested);

        let application_lifecycle = include_str!("../lib.rs");
        assert!(application_lifecycle.contains("shutdown_for_app_exit"));
        assert!(application_lifecycle.contains("WindowEvent::CloseRequested"));
        assert!(application_lifecycle.contains("RunEvent::ExitRequested"));
    }

    #[test]
    fn correction_2_ready_shutdown_closes_presentation_and_completes_inactive() {
        let mut state = ControllerState::default();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };
        state.phase = RuntimePhase::Ready;

        let sender = state
            .begin_control(1, Control::Shutdown)
            .expect("ready runtime should accept shutdown");
        sender.send(Control::Shutdown).unwrap();
        let control = commands.recv().unwrap();

        assert_eq!(control, Control::Shutdown);
        assert!(control.closes_presentation_intentionally());
        assert_eq!(state.phase, RuntimePhase::Stopping);
        assert!(state.complete_generation(1, RuntimePhase::Inactive, true, None));
        assert_eq!(state.snapshot().phase(), RuntimePhase::Inactive);
        assert!(state.snapshot().cleanup_complete);
        assert!(state.command_sender.is_none());
    }

    #[test]
    fn correction_2_starting_shutdown_is_observed_before_launch_and_cleans_up() {
        let mut state = ControllerState::default();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };

        let sender = state
            .begin_control(1, Control::Shutdown)
            .expect("starting runtime should accept shutdown");
        sender.send(Control::Shutdown).unwrap();

        let control = pending_control(&commands).expect("shutdown must cancel pending launch");
        assert_eq!(control, Control::Shutdown);
        assert_eq!(state.phase, RuntimePhase::Stopping);
        assert!(state.complete_generation(1, RuntimePhase::Inactive, true, None));
        assert_eq!(state.snapshot().phase(), RuntimePhase::Inactive);
        assert!(state.snapshot().cleanup_complete);
    }

    #[test]
    fn correction_2_repeated_shutdown_requests_are_idempotent() {
        let mut state = ControllerState::default();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };
        state.phase = RuntimePhase::Ready;

        let sender = state
            .begin_control(1, Control::Shutdown)
            .expect("first shutdown should be sent");
        assert!(state.begin_control(1, Control::Shutdown).is_none());
        assert!(state.begin_control(1, Control::Stop).is_none());

        sender.send(Control::Shutdown).unwrap();
        assert_eq!(commands.recv().unwrap(), Control::Shutdown);
        assert!(matches!(
            commands.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert_eq!(state.phase, RuntimePhase::Stopping);
    }

    #[test]
    fn correction_2_stopping_runtime_is_upgraded_once_to_application_shutdown() {
        let mut state = ControllerState::default();
        let commands = match state.begin_open().unwrap() {
            OpenDirective::Launch { commands, .. } => commands,
            _ => panic!("expected launch"),
        };
        state.phase = RuntimePhase::Ready;

        let stop = state
            .begin_control(1, Control::Stop)
            .expect("presentation stop should start cleanup");
        stop.send(Control::Stop).unwrap();
        let shutdown = state
            .begin_control(1, Control::Shutdown)
            .expect("main close should upgrade the existing cleanup");
        shutdown.send(Control::Shutdown).unwrap();
        assert!(state.begin_control(1, Control::Shutdown).is_none());

        assert_eq!(commands.recv().unwrap(), Control::Stop);
        assert_eq!(commands.recv().unwrap(), Control::Shutdown);
        assert!(state.shutdown_requested);
        assert_eq!(state.phase, RuntimePhase::Stopping);
    }

    #[test]
    fn correction_2_stop_and_shutdown_both_close_the_presentation_intentionally() {
        assert!(Control::Stop.closes_presentation_intentionally());
        assert!(Control::Shutdown.closes_presentation_intentionally());

        let worker = include_str!("controller.rs");
        assert!(worker.contains("control.closes_presentation_intentionally()"));
        assert!(worker.contains("PresentationHandle::close_intentionally"));
    }

    #[test]
    fn phase_4c_public_results_and_events_are_credential_free() {
        let controller = controller();
        let snapshot = controller.snapshot(&HarnessId::new("deepseek")).unwrap();
        let result = OpenHarnessResultDto {
            phase: snapshot.phase(),
            can_open: snapshot.can_open(),
            surface: controller.resolve_surface(snapshot),
        };
        let event = RuntimeChangedEventDto {
            harness_id: DEEPSEEK_ADAPTER_ID,
            phase: snapshot.phase(),
            can_open: snapshot.can_open(),
            surface: controller.resolve_surface(snapshot),
        };
        let serialized = format!(
            "{}{}",
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&event).unwrap()
        );

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
