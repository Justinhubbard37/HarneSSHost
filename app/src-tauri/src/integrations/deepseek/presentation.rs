use crate::integrations::deepseek::readiness::SensitiveReadyTarget;
use crate::runtime::driver::RuntimeGenerationReporter;
use std::fmt::{Debug, Display, Formatter};
use std::num::NonZeroU16;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::webview::NewWindowResponse;
use tauri::{Manager, WebviewUrl, WindowEvent};
use url::Url;

pub(crate) const DEEPSEEK_WINDOW_LABEL: &str = "deepseek-official-interface";
const DEEPSEEK_WINDOW_TITLE: &str = "DeepSeek - HarneSSHost";

fn deepseek_webview_incognito() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExactOwnedOrigin {
    port: NonZeroU16,
}

impl ExactOwnedOrigin {
    fn new(port: NonZeroU16) -> Self {
        Self { port }
    }

    fn allows(self, url: &Url) -> bool {
        url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port() == Some(self.port.get())
            && url.username().is_empty()
            && url.password().is_none()
    }
}

pub(crate) struct PresentationHandle {
    window: tauri::WebviewWindow,
    intentional_close: Arc<AtomicBool>,
}

impl PresentationHandle {
    pub(crate) fn close_intentionally(self) -> Result<(), PresenterError> {
        self.intentional_close.store(true, Ordering::Release);
        self.window.destroy().map_err(|_| PresenterError::close())
    }
}

pub(crate) fn present_official_deepseek_interface(
    app: &tauri::AppHandle,
    target: SensitiveReadyTarget,
    reporter: RuntimeGenerationReporter,
) -> Result<PresentationHandle, PresenterError> {
    let origin = ExactOwnedOrigin::new(target.port());
    let authenticated_url = target.into_authenticated_url();
    let navigation_origin = origin;

    let window = tauri::WebviewWindowBuilder::new(
        app,
        DEEPSEEK_WINDOW_LABEL,
        WebviewUrl::External(authenticated_url),
    )
    .title(DEEPSEEK_WINDOW_TITLE)
    .inner_size(1180.0, 780.0)
    .min_inner_size(720.0, 520.0)
    .incognito(deepseek_webview_incognito())
    .on_navigation(move |url| navigation_origin.allows(url))
    .on_new_window(|_, _| NewWindowResponse::Deny)
    .build()
    .map_err(|_| PresenterError::create())?;

    let intentional_close = Arc::new(AtomicBool::new(false));
    let close_requested = Arc::new(AtomicBool::new(false));
    let event_intentional_close = Arc::clone(&intentional_close);
    let event_close_requested = Arc::clone(&close_requested);
    let event_window = window.clone();
    let event_app = app.clone();

    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            if event_intentional_close.load(Ordering::Acquire) {
                return;
            }

            api.prevent_close();
            if !event_close_requested.swap(true, Ordering::AcqRel) {
                let _ = event_window.hide();
                reporter.presentation_closed(&event_app);
            }
        }
    });

    Ok(PresentationHandle {
        window,
        intentional_close,
    })
}

pub(crate) fn focus_existing_deepseek_interface(
    app: &tauri::AppHandle,
) -> Result<(), PresenterError> {
    let window = app
        .get_webview_window(DEEPSEEK_WINDOW_LABEL)
        .ok_or_else(PresenterError::missing)?;
    window.show().map_err(|_| PresenterError::focus())?;
    window.unminimize().map_err(|_| PresenterError::focus())?;
    window.set_focus().map_err(|_| PresenterError::focus())
}

pub(crate) struct PresenterError {
    code: &'static str,
    message: &'static str,
}

impl PresenterError {
    fn create() -> Self {
        Self {
            code: "deepseek.presenter-creation-failed",
            message: "The official DeepSeek interface window could not be created.",
        }
    }

    fn missing() -> Self {
        Self {
            code: "deepseek.presenter-window-missing",
            message: "The official DeepSeek interface window is not available.",
        }
    }

    fn focus() -> Self {
        Self {
            code: "deepseek.presenter-focus-failed",
            message: "The official DeepSeek interface window could not be focused.",
        }
    }

    fn close() -> Self {
        Self {
            code: "deepseek.presenter-close-failed",
            message: "The official DeepSeek interface window could not be closed.",
        }
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }
}

impl Debug for PresenterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PresenterError")
            .field("code", &self.code)
            .field("message", &self.message)
            .finish()
    }
}

impl Display for PresenterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PresenterError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_4c_exact_origin_policy_allows_only_the_owned_loopback_origin() {
        let policy = ExactOwnedOrigin::new(NonZeroU16::new(3080).unwrap());

        for allowed in [
            "http://127.0.0.1:3080/",
            "http://127.0.0.1:3080/chat/session?view=full#latest",
        ] {
            assert!(policy.allows(&Url::parse(allowed).unwrap()), "{allowed}");
        }

        for rejected in [
            "https://127.0.0.1:3080/",
            "http://localhost:3080/",
            "http://127.0.0.1:3081/",
            "http://127.0.0.1/",
            "http://user@127.0.0.1:3080/",
            "https://example.com/",
        ] {
            assert!(!policy.allows(&Url::parse(rejected).unwrap()), "{rejected}");
        }
    }

    #[test]
    fn phase_4c_remote_deepseek_window_has_no_tauri_capability() {
        let capability = include_str!("../../../capabilities/default.json");

        assert!(capability.contains(r#""windows": ["main"]"#));
        assert!(!capability.contains(DEEPSEEK_WINDOW_LABEL));
        assert!(!capability.contains("remote"));
    }

    #[test]
    fn phase_4c_presenter_contract_accepts_the_backend_private_sensitive_target() {
        let source = include_str!("presentation.rs");

        assert!(source.contains("target: SensitiveReadyTarget"));
        assert!(source.contains(".incognito(deepseek_webview_incognito())"));
        assert!(source.contains("NewWindowResponse::Deny"));
    }

    #[test]
    fn phase_4c_deepseek_presenter_requires_nonpersistent_webview() {
        assert!(deepseek_webview_incognito());
    }
}
