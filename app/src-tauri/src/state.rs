use crate::development_candidates;
use crate::harness::adapter::{DetectionContext, HarnessId};
use crate::harness::deepseek::DeepSeekAdapter;
use crate::harness::registry::{HarnessRegistry, RegistryError};
use crate::runtime::domain::{RuntimePhase, RuntimeState};
use std::collections::BTreeMap;
use std::sync::Arc;

pub(crate) struct AppState {
    pub(crate) registry: HarnessRegistry,
    pub(crate) detection_context: DetectionContext,
    runtime_states: BTreeMap<HarnessId, RuntimeState>,
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
        let runtime_states = registry
            .list()
            .into_iter()
            .map(|descriptor| (descriptor.id, RuntimeState::default()))
            .collect();

        Ok(Self {
            registry,
            detection_context,
            runtime_states,
        })
    }

    pub(crate) fn runtime_phase(&self, harness_id: &HarnessId) -> Option<RuntimePhase> {
        self.runtime_state(harness_id).map(RuntimeState::phase)
    }

    pub(crate) fn runtime_state(&self, harness_id: &HarnessId) -> Option<&RuntimeState> {
        self.runtime_states.get(harness_id)
    }

    #[cfg(test)]
    pub(crate) fn runtime_state_mut(
        &mut self,
        harness_id: &HarnessId,
    ) -> Option<&mut RuntimeState> {
        self.runtime_states.get_mut(harness_id)
    }
}
