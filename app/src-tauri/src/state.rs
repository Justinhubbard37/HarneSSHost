use crate::development_candidates;
use crate::harness::adapter::{DetectionContext, HarnessId};
use crate::harness::deepseek::DeepSeekAdapter;
use crate::harness::registry::{HarnessRegistry, RegistryError};
use crate::interface::DefaultInterfaceResolver;
use crate::runtime::controller::{RuntimeController, RuntimeSnapshot};
use std::sync::Arc;

pub(crate) struct AppState {
    pub(crate) registry: Arc<HarnessRegistry>,
    pub(crate) detection_context: DetectionContext,
    pub(crate) runtime_controller: RuntimeController,
}

impl AppState {
    pub(crate) fn new() -> Result<Self, RegistryError> {
        Self::with_detection_context(development_candidates::detection_context())
    }

    pub(crate) fn with_detection_context(
        detection_context: DetectionContext,
    ) -> Result<Self, RegistryError> {
        let mut registry = HarnessRegistry::default();
        registry.register(Arc::new(DeepSeekAdapter::new()))?;
        let registry = Arc::new(registry);
        let runtime_controller = RuntimeController::new(
            Arc::clone(&registry),
            detection_context.clone(),
            Arc::new(DefaultInterfaceResolver),
        );

        Ok(Self {
            registry,
            detection_context,
            runtime_controller,
        })
    }

    pub(crate) fn runtime_snapshot(&self, harness_id: &HarnessId) -> Option<RuntimeSnapshot> {
        self.runtime_controller.snapshot(harness_id)
    }
}
