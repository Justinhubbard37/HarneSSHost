use crate::runtime::driver::RuntimePresentationClass;
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InterfaceFacts {
    NoHarnessActive,
    Loading {
        harness_name: String,
    },
    OfficialInterfaceAvailable {
        harness_name: String,
        presentation: RuntimePresentationClass,
    },
    #[allow(dead_code)] // Preserved host seam; no current supported adapter produces it.
    Unavailable(String),
    Error(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum HostSurfaceKind {
    NoHarnessActive,
    Loading,
    OfficialInterfaceAvailable,
    Unavailable,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostSurfaceState {
    pub(crate) kind: HostSurfaceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) message: Option<String>,
}

pub(crate) trait InterfaceResolver: Send + Sync {
    fn resolve(&self, facts: InterfaceFacts) -> HostSurfaceState;
}

#[derive(Default)]
pub(crate) struct DefaultInterfaceResolver;

impl InterfaceResolver for DefaultInterfaceResolver {
    fn resolve(&self, facts: InterfaceFacts) -> HostSurfaceState {
        match facts {
            InterfaceFacts::NoHarnessActive => HostSurfaceState {
                kind: HostSurfaceKind::NoHarnessActive,
                message: None,
            },
            InterfaceFacts::Loading { harness_name } => HostSurfaceState {
                kind: HostSurfaceKind::Loading,
                message: Some(format!("Preparing the official {harness_name} interface.")),
            },
            InterfaceFacts::OfficialInterfaceAvailable {
                harness_name,
                presentation,
            } => HostSurfaceState {
                kind: HostSurfaceKind::OfficialInterfaceAvailable,
                message: Some(match presentation {
                    RuntimePresentationClass::OwnedIncognitoWebview => format!(
                        "The official {harness_name} interface is open in a separate HarneSSHost window."
                    ),
                    RuntimePresentationClass::PersistentExternalBrowser => format!(
                        "The official {harness_name} interface is open in an external browser."
                    ),
                }),
            },
            InterfaceFacts::Unavailable(message) => HostSurfaceState {
                kind: HostSurfaceKind::Unavailable,
                message: Some(message),
            },
            InterfaceFacts::Error(message) => HostSurfaceState {
                kind: HostSurfaceKind::Error,
                message: Some(message),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_every_phase_4a_host_surface() {
        let resolver = DefaultInterfaceResolver;

        assert_eq!(
            resolver.resolve(InterfaceFacts::NoHarnessActive).kind,
            HostSurfaceKind::NoHarnessActive
        );
        assert_eq!(
            resolver
                .resolve(InterfaceFacts::Loading {
                    harness_name: "Harness A".to_string(),
                })
                .kind,
            HostSurfaceKind::Loading
        );
        assert_eq!(
            resolver
                .resolve(InterfaceFacts::OfficialInterfaceAvailable {
                    harness_name: "Harness A".to_string(),
                    presentation: RuntimePresentationClass::OwnedIncognitoWebview,
                })
                .kind,
            HostSurfaceKind::OfficialInterfaceAvailable
        );
        assert_eq!(
            resolver
                .resolve(InterfaceFacts::Unavailable("Unavailable".to_string()))
                .kind,
            HostSurfaceKind::Unavailable
        );
        assert_eq!(
            resolver
                .resolve(InterfaceFacts::Error("Error".to_string()))
                .kind,
            HostSurfaceKind::Error
        );
    }

    #[test]
    fn phase_4c_official_interface_resolution_is_credential_free() {
        let surface =
            DefaultInterfaceResolver.resolve(InterfaceFacts::OfficialInterfaceAvailable {
                harness_name: "Harness A".to_string(),
                presentation: RuntimePresentationClass::OwnedIncognitoWebview,
            });
        let json = serde_json::to_string(&surface).unwrap();

        assert_eq!(surface.kind, HostSurfaceKind::OfficialInterfaceAvailable);
        for forbidden in ["token", "authenticatedUrl", "?token=", "127.0.0.1"] {
            assert!(!json.contains(forbidden), "unexpected value: {forbidden}");
        }
    }
}
