use crate::development_candidates;
use crate::harness::adapter::DetectionContext;
use crate::harness::deepseek::DeepSeekAdapter;
use crate::harness::registry::{HarnessRegistry, RegistryError};
use crate::interface::DefaultInterfaceResolver;
use std::sync::Arc;

pub(crate) struct AppState {
    pub(crate) registry: HarnessRegistry,
    pub(crate) detection_context: DetectionContext,
    pub(crate) interface_resolver: DefaultInterfaceResolver,
}

impl AppState {
    pub(crate) fn new() -> Result<Self, RegistryError> {
        let mut registry = HarnessRegistry::default();
        registry.register(Arc::new(DeepSeekAdapter::new()))?;

        Ok(Self {
            registry,
            detection_context: development_candidates::detection_context(),
            interface_resolver: DefaultInterfaceResolver,
        })
    }
}
