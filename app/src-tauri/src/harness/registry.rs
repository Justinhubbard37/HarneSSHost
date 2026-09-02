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
    use crate::harness::adapter::{
        AdapterError, DetectionContext, DetectionReport, HarnessDescriptor, VersionReport,
    };
    use crate::harness::capability::CapabilityManifest;

    struct FakeAdapter {
        descriptor: HarnessDescriptor,
    }

    impl FakeAdapter {
        fn new(id: &str) -> Self {
            Self {
                descriptor: HarnessDescriptor {
                    id: HarnessId::new(id),
                    display_name: format!("Harness {id}"),
                    description: "Generic registry fixture.".to_string(),
                    official_interfaces: Vec::new(),
                    supported_topologies: Vec::new(),
                },
            }
        }
    }

    impl HarnessAdapter for FakeAdapter {
        fn descriptor(&self) -> &HarnessDescriptor {
            &self.descriptor
        }

        fn detect(&self, _context: &DetectionContext) -> Result<DetectionReport, AdapterError> {
            unreachable!("registry tests do not perform detection")
        }

        fn version(
            &self,
            _installation: &crate::harness::adapter::DetectedInstallation,
        ) -> Result<VersionReport, AdapterError> {
            unreachable!("registry tests do not inspect versions")
        }

        fn capability_manifest(&self, _version: Option<&VersionReport>) -> CapabilityManifest {
            unreachable!("registry tests do not inspect capabilities")
        }
    }

    #[test]
    fn registers_lists_and_resolves_an_adapter() {
        let mut registry = HarnessRegistry::default();
        registry
            .register(Arc::new(FakeAdapter::new("harness-a")))
            .unwrap();

        let descriptors = registry.list();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].id.as_str(), "harness-a");
        assert!(registry.get(&HarnessId::new("harness-a")).is_some());
    }

    #[test]
    fn rejects_duplicate_adapter_ids() {
        let mut registry = HarnessRegistry::default();
        registry
            .register(Arc::new(FakeAdapter::new("harness-a")))
            .unwrap();

        let error = registry
            .register(Arc::new(FakeAdapter::new("harness-a")))
            .unwrap_err();
        assert_eq!(
            error,
            RegistryError::DuplicateAdapter(HarnessId::new("harness-a"))
        );
    }

    #[test]
    fn lists_adapters_in_stable_id_order() {
        let mut registry = HarnessRegistry::default();
        registry
            .register(Arc::new(FakeAdapter::new("harness-b")))
            .unwrap();
        registry
            .register(Arc::new(FakeAdapter::new("harness-a")))
            .unwrap();

        let descriptors = registry.list();
        assert_eq!(descriptors.len(), 2);
        assert_eq!(descriptors[0].id.as_str(), "harness-a");
        assert_eq!(descriptors[1].id.as_str(), "harness-b");
        assert!(registry.get(&HarnessId::new("harness-a")).is_some());
        assert!(registry.get(&HarnessId::new("harness-b")).is_some());
    }
}
