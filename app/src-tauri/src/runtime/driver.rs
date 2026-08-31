use crate::harness::adapter::{ExecutionTopology, HarnessId};
use crate::runtime::domain::RuntimePhase;
use serde::Serialize;
use std::fmt::{Debug, Display, Formatter};
use std::sync::{mpsc, Arc};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RuntimeOwnershipClass {
    WindowsJob,
    #[allow(dead_code)] // Concrete OC-2/accepted-plan boundary; implemented in a later gate.
    SystemdUserServiceCgroup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RuntimeReadinessClass {
    StdoutLaunchToken,
    #[allow(dead_code)] // Concrete OC-2/accepted-plan boundary; implemented in a later gate.
    AuthenticatedHttp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RuntimeAuthenticationClass {
    LaunchTokenSessionCookie,
    #[allow(dead_code)] // Concrete OC-2/accepted-plan boundary; implemented in a later gate.
    BasicAuthentication,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RuntimePresentationClass {
    OwnedIncognitoWebview,
    #[allow(dead_code)] // Concrete OC-2/accepted-plan boundary; implemented in a later gate.
    PersistentExternalBrowser,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PresentationCloseSemantics {
    StopRuntime,
    #[allow(dead_code)] // Concrete OC-2/accepted-plan boundary; implemented in a later gate.
    RuntimeContinues,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeDriverMetadata {
    pub(crate) topology: ExecutionTopology,
    pub(crate) ownership: RuntimeOwnershipClass,
    pub(crate) readiness: RuntimeReadinessClass,
    pub(crate) authentication: RuntimeAuthenticationClass,
    pub(crate) presentation: RuntimePresentationClass,
    pub(crate) presentation_close: PresentationCloseSemantics,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OwnedRuntimeIdentity {
    harness_id: HarnessId,
    generation: u64,
}

impl OwnedRuntimeIdentity {
    pub(crate) fn new(harness_id: HarnessId, generation: u64) -> Self {
        Self {
            harness_id,
            generation,
        }
    }

    pub(crate) fn harness_id(&self) -> &HarnessId {
        &self.harness_id
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

pub(crate) trait OwnedHarnessRuntime {
    fn identity(&self) -> &OwnedRuntimeIdentity;
    fn stop(&mut self) -> Result<(), RuntimeFailure>;
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct RuntimeFailure {
    harness_id: HarnessId,
    code: &'static str,
}

impl RuntimeFailure {
    pub(crate) fn new(harness_id: HarnessId, code: &'static str) -> Self {
        Self { harness_id, code }
    }

    pub(crate) fn harness_id(&self) -> &HarnessId {
        &self.harness_id
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }
}

impl Debug for RuntimeFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeFailure")
            .field("harness_id", &self.harness_id)
            .field("code", &self.code)
            .finish()
    }
}

impl Display for RuntimeFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.harness_id.as_str(), self.code)
    }
}

impl std::error::Error for RuntimeFailure {}

pub(crate) struct RuntimeCompletion {
    phase: RuntimePhase,
    cleanup_complete: bool,
    failure: Option<RuntimeFailure>,
}

impl RuntimeCompletion {
    pub(crate) fn inactive() -> Self {
        Self {
            phase: RuntimePhase::Inactive,
            cleanup_complete: true,
            failure: None,
        }
    }

    pub(crate) fn failed(failure: RuntimeFailure, cleanup_complete: bool) -> Self {
        Self {
            phase: RuntimePhase::Failed,
            cleanup_complete,
            failure: Some(failure),
        }
    }

    pub(crate) fn phase(&self) -> RuntimePhase {
        self.phase
    }

    pub(crate) fn cleanup_complete(&self) -> bool {
        self.cleanup_complete
    }

    pub(crate) fn failure(&self) -> Option<&RuntimeFailure> {
        self.failure.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeControl {
    Stop,
    Shutdown,
}

pub(crate) trait RuntimeEventSink: Send + Sync {
    fn publish_ready(&self, app: &tauri::AppHandle, harness_id: &HarnessId, generation: u64);

    fn presentation_closed(&self, app: &tauri::AppHandle, harness_id: &HarnessId, generation: u64);
}

#[derive(Clone)]
pub(crate) struct RuntimeGenerationReporter {
    sink: Arc<dyn RuntimeEventSink>,
    harness_id: HarnessId,
    generation: u64,
}

impl RuntimeGenerationReporter {
    pub(crate) fn new(
        sink: Arc<dyn RuntimeEventSink>,
        harness_id: HarnessId,
        generation: u64,
    ) -> Self {
        Self {
            sink,
            harness_id,
            generation,
        }
    }

    pub(crate) fn publish_ready(&self, app: &tauri::AppHandle) {
        self.sink
            .publish_ready(app, &self.harness_id, self.generation);
    }

    pub(crate) fn presentation_closed(&self, app: &tauri::AppHandle) {
        self.sink
            .presentation_closed(app, &self.harness_id, self.generation);
    }
}

pub(crate) struct RuntimeRunContext {
    app: tauri::AppHandle,
    identity: OwnedRuntimeIdentity,
    commands: mpsc::Receiver<RuntimeControl>,
    reporter: RuntimeGenerationReporter,
}

impl RuntimeRunContext {
    pub(crate) fn new(
        app: tauri::AppHandle,
        identity: OwnedRuntimeIdentity,
        commands: mpsc::Receiver<RuntimeControl>,
        reporter: RuntimeGenerationReporter,
    ) -> Self {
        Self {
            app,
            identity,
            commands,
            reporter,
        }
    }

    pub(crate) fn app(&self) -> &tauri::AppHandle {
        &self.app
    }

    pub(crate) fn identity(&self) -> &OwnedRuntimeIdentity {
        &self.identity
    }

    pub(crate) fn reporter(&self) -> &RuntimeGenerationReporter {
        &self.reporter
    }

    pub(crate) fn pending_control(&self) -> Option<RuntimeControl> {
        match self.commands.try_recv() {
            Ok(control) => Some(control),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(RuntimeControl::Shutdown),
        }
    }
}

pub(crate) trait HarnessRuntimeDriver: Send + Sync {
    fn harness_id(&self) -> &HarnessId;
    fn presentation_name(&self) -> &str;
    fn metadata(&self) -> RuntimeDriverMetadata;
    fn run_generation(&self, context: RuntimeRunContext) -> RuntimeCompletion;
    fn focus_presentation(&self, app: &tauri::AppHandle) -> Result<(), RuntimeFailure>;
}
