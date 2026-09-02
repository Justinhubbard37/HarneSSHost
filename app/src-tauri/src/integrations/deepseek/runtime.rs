use crate::harness::adapter::{DetectionContext, ExecutionTopology, HarnessId};
use crate::harness::registry::HarnessRegistry;
use crate::integrations::deepseek::adapter::DEEPSEEK_ADAPTER_ID;
use crate::integrations::deepseek::presentation::{
    focus_existing_deepseek_interface, present_official_deepseek_interface, PresentationHandle,
};
use crate::integrations::deepseek::windows::{
    prepare_deepseek_launch, DeepSeekLaunchPlan, DeepSeekOwnedRuntime, DeepSeekReadinessUpdate,
};
use crate::runtime::domain::RuntimePhase;
use crate::runtime::driver::{
    HarnessRuntimeDriver, OwnedHarnessRuntime, OwnedRuntimeIdentity, PresentationCloseSemantics,
    RuntimeAuthenticationClass, RuntimeCompletion, RuntimeDriverMetadata, RuntimeFailure,
    RuntimeOwnershipClass, RuntimePresentationClass, RuntimeReadinessClass, RuntimeRunContext,
};
use std::fs::File;
use std::io::Read;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

const OUTPUT_CHANNEL_CAPACITY: usize = 64;
const OUTPUT_CHUNK_BYTES: usize = 2_048;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(crate) struct DeepSeekRuntimeDriver {
    harness_id: HarnessId,
    presentation_name: String,
    registry: Arc<HarnessRegistry>,
    detection_context: DetectionContext,
}

impl DeepSeekRuntimeDriver {
    pub(crate) fn new(registry: Arc<HarnessRegistry>, detection_context: DetectionContext) -> Self {
        let harness_id = HarnessId::new(DEEPSEEK_ADAPTER_ID);
        registry
            .get(&harness_id)
            .expect("the DeepSeek runtime driver requires its accepted adapter");
        Self {
            harness_id,
            presentation_name: "DeepSeek".to_string(),
            registry,
            detection_context,
        }
    }

    fn failure(&self, code: &'static str) -> RuntimeFailure {
        RuntimeFailure::new(self.harness_id.clone(), code)
    }

    fn launch_owned(
        &self,
        plan: DeepSeekLaunchPlan,
        identity: OwnedRuntimeIdentity,
    ) -> Result<DeepSeekOwnedRuntimeHandle, RuntimeFailure> {
        let runtime = DeepSeekOwnedRuntime::launch(plan, identity.generation())
            .map_err(|error| self.failure(error.code()))?;
        Ok(DeepSeekOwnedRuntimeHandle { identity, runtime })
    }
}

impl HarnessRuntimeDriver for DeepSeekRuntimeDriver {
    fn harness_id(&self) -> &HarnessId {
        &self.harness_id
    }

    fn presentation_name(&self) -> &str {
        &self.presentation_name
    }

    fn metadata(&self) -> RuntimeDriverMetadata {
        RuntimeDriverMetadata {
            topology: ExecutionTopology::NativeWindows,
            ownership: RuntimeOwnershipClass::WindowsJob,
            readiness: RuntimeReadinessClass::StdoutLaunchToken,
            authentication: RuntimeAuthenticationClass::LaunchTokenSessionCookie,
            presentation: RuntimePresentationClass::OwnedIncognitoWebview,
            presentation_close: PresentationCloseSemantics::StopRuntime,
        }
    }

    fn run_generation(&self, context: RuntimeRunContext) -> RuntimeCompletion {
        debug_assert_eq!(context.identity().harness_id(), &self.harness_id);
        if context.pending_control().is_some() {
            return RuntimeCompletion::inactive();
        }

        let plan = match prepare_deepseek_launch(&self.registry, &self.detection_context) {
            Ok(plan) => plan,
            Err(error) => return RuntimeCompletion::failed(self.failure(error.code()), true),
        };

        if context.pending_control().is_some() {
            return RuntimeCompletion::inactive();
        }

        let mut runtime = match self.launch_owned(plan, context.identity().clone()) {
            Ok(runtime) => runtime,
            Err(failure) => return RuntimeCompletion::failed(failure, true),
        };
        debug_assert_eq!(runtime.identity(), context.identity());
        let output = start_output_pumps(&mut runtime.runtime);
        let mut presentation: Option<PresentationHandle> = None;

        loop {
            if let Some(_control) = context.pending_control() {
                let close_result = presentation
                    .take()
                    .map(PresentationHandle::close_intentionally)
                    .transpose()
                    .map(|_| ());
                let stopped = runtime.stop();
                return match (stopped, close_result) {
                    (Ok(()), Ok(())) => RuntimeCompletion::inactive(),
                    (Err(failure), _) => RuntimeCompletion::failed(failure, false),
                    (Ok(()), Err(error)) => {
                        RuntimeCompletion::failed(self.failure(error.code()), true)
                    }
                };
            }

            match output.recv_timeout(WORKER_POLL_INTERVAL) {
                Ok(OutputUpdate::Stdout(chunk)) => match runtime.runtime.ingest_stdout(&chunk) {
                    Ok(DeepSeekReadinessUpdate::Pending) => {}
                    Ok(DeepSeekReadinessUpdate::Ready { .. }) => {
                        let Some(target) = runtime.runtime.take_ready_target() else {
                            let cleaned = runtime
                                .runtime
                                .fail_owned_runtime(
                                    "deepseek.ready-target-unavailable",
                                    "The private DeepSeek readiness target was unavailable.",
                                )
                                .is_ok();
                            return RuntimeCompletion::failed(
                                self.failure("deepseek.ready-target-unavailable"),
                                cleaned,
                            );
                        };
                        match present_official_deepseek_interface(
                            context.app(),
                            target,
                            context.reporter().clone(),
                        ) {
                            Ok(window) => {
                                presentation = Some(window);
                                context.reporter().publish_ready(context.app());
                            }
                            Err(error) => {
                                let code = error.code();
                                let cleaned = runtime
                                    .runtime
                                    .fail_owned_runtime(
                                        code,
                                        "The official DeepSeek interface could not be presented.",
                                    )
                                    .is_ok();
                                return RuntimeCompletion::failed(self.failure(code), cleaned);
                            }
                        }
                    }
                    Ok(DeepSeekReadinessUpdate::Failed { code }) => {
                        close_invalid_presentation(presentation.take());
                        return RuntimeCompletion::failed(self.failure(code), true);
                    }
                    Err(error) => {
                        close_invalid_presentation(presentation.take());
                        return RuntimeCompletion::failed(self.failure(error.code()), false);
                    }
                },
                Ok(OutputUpdate::Stderr(chunk)) => runtime.runtime.record_sanitized_stderr(&chunk),
                Ok(OutputUpdate::ReadFailed) => {
                    let code = "deepseek.output-read-failed";
                    let cleaned = runtime
                        .runtime
                        .fail_owned_runtime(
                            code,
                            "Owned DeepSeek output could not be observed safely.",
                        )
                        .is_ok();
                    close_invalid_presentation(presentation.take());
                    return RuntimeCompletion::failed(self.failure(code), cleaned);
                }
                Ok(OutputUpdate::Closed) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }

            match runtime.runtime.wait_for_root_exit(Duration::ZERO) {
                Ok(false) => {}
                Ok(true) => {
                    let code = if runtime.runtime.phase() == RuntimePhase::Ready {
                        "deepseek.runtime-exited-unexpectedly"
                    } else {
                        "deepseek.process-exited-before-readiness"
                    };
                    let cleaned = runtime.runtime.reconcile_unexpected_root_exit().is_ok();
                    close_invalid_presentation(presentation.take());
                    return RuntimeCompletion::failed(self.failure(code), cleaned);
                }
                Err(error) => {
                    let code = error.code();
                    let cleaned = runtime
                        .runtime
                        .fail_owned_runtime(
                            code,
                            "The owned DeepSeek process could not be observed safely.",
                        )
                        .is_ok();
                    close_invalid_presentation(presentation.take());
                    return RuntimeCompletion::failed(self.failure(code), cleaned);
                }
            }
        }
    }

    fn focus_presentation(&self, app: &tauri::AppHandle) -> Result<(), RuntimeFailure> {
        focus_existing_deepseek_interface(app).map_err(|error| self.failure(error.code()))
    }
}

struct DeepSeekOwnedRuntimeHandle {
    identity: OwnedRuntimeIdentity,
    runtime: DeepSeekOwnedRuntime,
}

impl OwnedHarnessRuntime for DeepSeekOwnedRuntimeHandle {
    fn identity(&self) -> &OwnedRuntimeIdentity {
        &self.identity
    }

    fn stop(&mut self) -> Result<(), RuntimeFailure> {
        self.runtime
            .stop_owned()
            .map_err(|error| RuntimeFailure::new(self.identity.harness_id().clone(), error.code()))
    }
}

enum OutputUpdate {
    Stdout(Vec<u8>),
    Stderr(String),
    ReadFailed,
    Closed,
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
    #[test]
    fn presentation_controls_close_the_window_and_stop_the_owned_runtime() {
        let worker = include_str!("runtime.rs");
        assert!(worker.contains("PresentationHandle::close_intentionally"));
        assert!(worker.contains("runtime.stop()"));
    }
}
