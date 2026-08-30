use crate::harness::adapter::{HarnessAdapter, HarnessDescriptor, HarnessId};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct HarnessRegistry {
    adapters: BTreeMap<HarnessId, Arc<dyn HarnessAdapter>>,
}

impl HarnessRegistry {
    pub(crate) fn register(
        &mut self,
        adapter: Arc<dyn HarnessAdapter>,
    ) -> Result<(), RegistryError> {
        let id = adapter.descriptor().id.clone();
        if self.adapters.contains_key(&id) {
            return Err(RegistryError::DuplicateAdapter(id));
        }
        self.adapters.insert(id, adapter);
        Ok(())
    }

    pub(crate) fn list(&self) -> Vec<HarnessDescriptor> {
        self.adapters
            .values()
            .map(|adapter| adapter.descriptor().clone())
            .collect()
    }

    pub(crate) fn get(&self, id: &HarnessId) -> Option<Arc<dyn HarnessAdapter>> {
        self.adapters.get(id).cloned()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RegistryError {
    DuplicateAdapter(HarnessId),
}

impl Display for RegistryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateAdapter(id) => {
                write!(formatter, "duplicate harness adapter: {}", id.as_str())
            }
        }
    }
}

impl Error for RegistryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::deepseek::DeepSeekAdapter;
    use crate::harness::opencode::OpenCodeAdapter;

    #[test]
    fn registers_lists_and_resolves_an_adapter() {
        let mut registry = HarnessRegistry::default();
        registry.register(Arc::new(DeepSeekAdapter::new())).unwrap();

        let descriptors = registry.list();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].id.as_str(), "deepseek");
        assert!(registry.get(&HarnessId::new("deepseek")).is_some());
    }

    #[test]
    fn rejects_duplicate_adapter_ids() {
        let mut registry = HarnessRegistry::default();
        registry.register(Arc::new(DeepSeekAdapter::new())).unwrap();

        let error = registry
            .register(Arc::new(DeepSeekAdapter::new()))
            .unwrap_err();
        assert_eq!(
            error,
            RegistryError::DuplicateAdapter(HarnessId::new("deepseek"))
        );
    }

    #[test]
    fn registers_both_concrete_harness_descriptors() {
        let mut registry = HarnessRegistry::default();
        registry.register(Arc::new(DeepSeekAdapter::new())).unwrap();
        registry.register(Arc::new(OpenCodeAdapter::new())).unwrap();

        let descriptors = registry.list();
        assert_eq!(descriptors.len(), 2);
        assert_eq!(descriptors[0].id.as_str(), "deepseek");
        assert_eq!(descriptors[1].id.as_str(), "opencode");
        assert!(registry.get(&HarnessId::new("deepseek")).is_some());
        assert!(registry.get(&HarnessId::new("opencode")).is_some());
    }
}
