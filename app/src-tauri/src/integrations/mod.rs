mod deepseek;
mod opencode;

use crate::harness::adapter::DetectionContext;
#[cfg(test)]
use crate::harness::adapter::HarnessId;
use crate::harness::registry::{HarnessRegistry, RegistryError};
use crate::runtime::driver::HarnessRuntimeDriver;
use crate::state::AppState;
use std::sync::Arc;

pub(crate) fn app_state() -> Result<AppState, RegistryError> {
    app_state_with_detection_context(detection_context())
}

fn detection_context() -> DetectionContext {
    #[allow(unused_mut)]
    let mut candidates = deepseek::development::candidates();

    #[cfg(not(test))]
    {
        candidates.extend(opencode::adapter::machine_candidates());
    }

    DetectionContext::new(candidates)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn app_state_with_detection_context(
    detection_context: DetectionContext,
) -> Result<AppState, RegistryError> {
    let mut registry = HarnessRegistry::default();
    registry.register(Arc::new(deepseek::adapter::DeepSeekAdapter::new()))?;
    registry.register(Arc::new(opencode::adapter::OpenCodeAdapter::new()))?;
    let registry = Arc::new(registry);
    let runtime_drivers: Vec<Arc<dyn HarnessRuntimeDriver>> =
        vec![Arc::new(deepseek::runtime::DeepSeekRuntimeDriver::new(
            Arc::clone(&registry),
            detection_context.clone(),
        ))];

    Ok(AppState::new(registry, detection_context, runtime_drivers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipping_integrations_compose_at_one_boundary() {
        let state = app_state_with_detection_context(DetectionContext::default()).unwrap();
        let descriptors = state.registry.list();

        assert_eq!(descriptors.len(), 2);
        assert_eq!(
            descriptors[0].id.as_str(),
            deepseek::adapter::DEEPSEEK_ADAPTER_ID
        );
        assert_eq!(
            descriptors[1].id.as_str(),
            opencode::adapter::OPENCODE_ADAPTER_ID
        );
        assert!(state
            .runtime_snapshot(&HarnessId::new(deepseek::adapter::DEEPSEEK_ADAPTER_ID))
            .is_some());
        assert!(state
            .runtime_snapshot(&HarnessId::new(opencode::adapter::OPENCODE_ADAPTER_ID))
            .is_none());
    }
}
