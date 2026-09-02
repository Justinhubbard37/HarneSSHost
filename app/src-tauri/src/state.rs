use crate::harness::adapter::{DetectionContext, HarnessId};
use crate::harness::registry::HarnessRegistry;
use crate::interface::DefaultInterfaceResolver;
use crate::runtime::controller::{RuntimeController, RuntimeSnapshot};
use crate::runtime::driver::HarnessRuntimeDriver;
use std::sync::Arc;

pub(crate) struct AppState {
    pub(crate) registry: Arc<HarnessRegistry>,
    pub(crate) detection_context: DetectionContext,
    pub(crate) runtime_controller: RuntimeController,
}

impl AppState {
    pub(crate) fn new(
        registry: Arc<HarnessRegistry>,
        detection_context: DetectionContext,
        runtime_drivers: Vec<Arc<dyn HarnessRuntimeDriver>>,
    ) -> Self {
        let runtime_controller =
            RuntimeController::new(runtime_drivers, Arc::new(DefaultInterfaceResolver));

        Self {
            registry,
            detection_context,
            runtime_controller,
        }
    }

    pub(crate) fn runtime_snapshot(&self, harness_id: &HarnessId) -> Option<RuntimeSnapshot> {
        self.runtime_controller.snapshot(harness_id)
    }
}
