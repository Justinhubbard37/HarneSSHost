use super::{
    append_quoted_argument, launch_contained, OwnedWindowsRuntime, WindowsCommandLine,
    WindowsProcessSpec, WindowsSupervisorError,
};
use crate::harness::adapter::{
    CompatibilityState, DetectedInstallation, DetectionContext, DetectionReport, HarnessId,
};
use crate::harness::deepseek::DEEPSEEK_ADAPTER_ID;
use crate::harness::registry::HarnessRegistry;
use crate::runtime::diagnostics::DiagnosticBuffer;
use crate::runtime::domain::{RuntimeOwnership, RuntimePhase, RuntimeState};
use crate::runtime::readiness::{DeepSeekReadinessParser, SensitiveReadyTarget};
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::mem::size_of;
use std::num::NonZeroU16;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::time::Duration;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

const COMMAND_PROCESSOR_NAME: &str = "cmd.exe";
const COREPACK_SHIM_NAME: &str = "corepack.cmd";
const NODE_REGISTRY_KEY: &str = "SOFTWARE\\Node.js";
const NODE_INSTALL_PATH_VALUE: &str = "InstallPath";
const MAX_SYSTEM_PATH_UNITS: usize = 32_768;
const MAX_REGISTRY_STRING_BYTES: u32 = 65_536;
const OWNED_STOP_TIMEOUT: Duration = Duration::from_secs(10);
const DIAGNOSTIC_RECORDS: usize = 32;
const DIAGNOSTIC_TOTAL_BYTES: usize = 8_192;
const DIAGNOSTIC_RECORD_BYTES: usize = 512;
const CMD_SWITCHES: [&str; 4] = ["/d", "/s", "/v:off", "/c"];
const FIXED_DEEPSEEK_ARGUMENTS: [&str; 8] = [
    "pnpm",
    "dsh",
    "web",
    "--no-open",
    "--host",
    "127.0.0.1",
    "--port",
    "0",
];

pub(crate) struct DeepSeekLaunchPlan {
    specification: WindowsProcessSpec,
}

impl DeepSeekLaunchPlan {
    fn into_process_specification(self) -> WindowsProcessSpec {
        self.specification
    }
}

struct ResolvedWindowsTools {
    command_processor: PathBuf,
    corepack_shim: PathBuf,
}

impl ResolvedWindowsTools {
    fn discover() -> Result<Self, DeepSeekRuntimeError> {
        let system_directory = resolve_system_directory()?;
        let command_processor = system_directory.join(COMMAND_PROCESSOR_NAME);
        validate_named_file(
            &command_processor,
            COMMAND_PROCESSOR_NAME,
            "deepseek.command-processor-unavailable",
            "The trusted Windows command processor is unavailable.",
        )?;

        let node_installation = read_machine_node_installation()?;
        let corepack_shim = node_installation.join(COREPACK_SHIM_NAME);
        validate_named_file(
            &corepack_shim,
            COREPACK_SHIM_NAME,
            "deepseek.corepack-shim-unavailable",
            "The registered Corepack command shim is unavailable.",
        )?;

        let canonical_installation = std::fs::canonicalize(&node_installation).map_err(|_| {
            DeepSeekRuntimeError::new(
                "deepseek.node-installation-unavailable",
                "The registered Node.js installation is unavailable.",
            )
        })?;
        let canonical_shim = std::fs::canonicalize(&corepack_shim).map_err(|_| {
            DeepSeekRuntimeError::new(
                "deepseek.corepack-shim-unavailable",
                "The registered Corepack command shim is unavailable.",
            )
        })?;
        if canonical_shim.parent() != Some(canonical_installation.as_path()) {
            return Err(DeepSeekRuntimeError::new(
                "deepseek.corepack-shim-outside-installation",
                "The Corepack command shim is outside the registered Node.js installation.",
            ));
        }

        Ok(Self {
            command_processor,
            corepack_shim,
        })
    }

    #[cfg(test)]
    fn for_test(
        command_processor: PathBuf,
        corepack_shim: PathBuf,
    ) -> Result<Self, DeepSeekRuntimeError> {
        validate_named_file(
            &command_processor,
            COMMAND_PROCESSOR_NAME,
            "deepseek.command-processor-unavailable",
            "The trusted Windows command processor is unavailable.",
        )?;
        validate_named_file(
            &corepack_shim,
            COREPACK_SHIM_NAME,
            "deepseek.corepack-shim-unavailable",
            "The registered Corepack command shim is unavailable.",
        )?;
        Ok(Self {
            command_processor,
            corepack_shim,
        })
    }
}

pub(crate) fn prepare_deepseek_launch(
    registry: &HarnessRegistry,
    detection_context: &DetectionContext,
) -> Result<DeepSeekLaunchPlan, DeepSeekRuntimeError> {
    let installation = verified_installation(registry, detection_context)?;
    let tools = ResolvedWindowsTools::discover()?;
    build_launch_plan(installation, tools)
}

#[cfg(test)]
fn prepare_deepseek_launch_with_tools(
    registry: &HarnessRegistry,
    detection_context: &DetectionContext,
    tools: ResolvedWindowsTools,
) -> Result<DeepSeekLaunchPlan, DeepSeekRuntimeError> {
    let installation = verified_installation(registry, detection_context)?;
    build_launch_plan(installation, tools)
}

fn verified_installation(
    registry: &HarnessRegistry,
    detection_context: &DetectionContext,
) -> Result<DetectedInstallation, DeepSeekRuntimeError> {
    let adapter = registry
        .get(&HarnessId::new(DEEPSEEK_ADAPTER_ID))
        .ok_or_else(|| {
            DeepSeekRuntimeError::new(
                "deepseek.adapter-unavailable",
                "The DeepSeek adapter is not registered.",
            )
        })?;

    let installation = match adapter.detect(detection_context) {
        Ok(DetectionReport::Detected(installation)) => installation,
        Ok(DetectionReport::NotFound { .. }) => {
            return Err(DeepSeekRuntimeError::new(
                "deepseek.launch-installation-not-found",
                "A verified DeepSeek installation is required for launch.",
            ));
        }
        Ok(
            DetectionReport::Invalid { .. }
            | DetectionReport::Rejected { .. }
            | DetectionReport::Error { .. },
        )
        | Err(_) => {
            return Err(DeepSeekRuntimeError::new(
                "deepseek.launch-installation-invalid",
                "The local DeepSeek installation did not pass verification.",
            ));
        }
    };

    let version = adapter.version(&installation).map_err(|_| {
        DeepSeekRuntimeError::new(
            "deepseek.launch-version-invalid",
            "The local DeepSeek version metadata did not pass verification.",
        )
    })?;
    if version.compatibility != CompatibilityState::TestedVersionMatch || version.start_blocked {
        return Err(DeepSeekRuntimeError::new(
            "deepseek.launch-version-unsupported",
            "The local DeepSeek version is not approved for launch.",
        ));
    }

    Ok(installation)
}

fn build_launch_plan(
    installation: DetectedInstallation,
    tools: ResolvedWindowsTools,
) -> Result<DeepSeekLaunchPlan, DeepSeekRuntimeError> {
    let command_line = build_fixed_command_line(&tools.command_processor, &tools.corepack_shim)?;
    let mut specification = WindowsProcessSpec::new(tools.command_processor, Vec::new())
        .with_working_directory(installation.path().to_path_buf());
    specification.command_line = WindowsCommandLine::TrustedPrequoted(command_line);
    Ok(DeepSeekLaunchPlan { specification })
}

fn build_fixed_command_line(
    command_processor: &Path,
    corepack_shim: &Path,
) -> Result<Vec<u16>, DeepSeekRuntimeError> {
    validate_shell_script_path(corepack_shim)?;
    let mut command_line = Vec::new();
    append_quoted_argument(&mut command_line, command_processor.as_os_str()).map_err(|_| {
        DeepSeekRuntimeError::new(
            "deepseek.command-processor-path-invalid",
            "The trusted Windows command processor path is invalid.",
        )
    })?;
    for switch in &CMD_SWITCHES[..CMD_SWITCHES.len() - 1] {
        push_ascii_argument(&mut command_line, switch);
    }
    push_ascii_argument(&mut command_line, CMD_SWITCHES[CMD_SWITCHES.len() - 1]);

    command_line.push(b' ' as u16);
    command_line.push(b'"' as u16);
    command_line.push(b'"' as u16);
    command_line.extend(corepack_shim.as_os_str().encode_wide());
    command_line.push(b'"' as u16);
    for argument in FIXED_DEEPSEEK_ARGUMENTS {
        push_ascii_argument(&mut command_line, argument);
    }
    command_line.push(b'"' as u16);
    Ok(command_line)
}

fn push_ascii_argument(command_line: &mut Vec<u16>, argument: &str) {
    command_line.push(b' ' as u16);
    command_line.extend(argument.encode_utf16());
}

fn validate_shell_script_path(path: &Path) -> Result<(), DeepSeekRuntimeError> {
    const FORBIDDEN: [u16; 11] = [
        b'"' as u16,
        b'%' as u16,
        b'!' as u16,
        b'^' as u16,
        b'&' as u16,
        b'|' as u16,
        b'<' as u16,
        b'>' as u16,
        b'(' as u16,
        b')' as u16,
        0,
    ];
    if path
        .as_os_str()
        .encode_wide()
        .any(|unit| FORBIDDEN.contains(&unit) || matches!(unit, 0x0a | 0x0d))
    {
        return Err(DeepSeekRuntimeError::new(
            "deepseek.corepack-shim-path-unsafe",
            "The Corepack command shim path is unsafe for the fixed command boundary.",
        ));
    }
    Ok(())
}

fn resolve_system_directory() -> Result<PathBuf, DeepSeekRuntimeError> {
    let mut buffer = vec![0u16; MAX_SYSTEM_PATH_UNITS];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(DeepSeekRuntimeError::new(
            "deepseek.system-directory-unavailable",
            "The trusted Windows system directory is unavailable.",
        ));
    }
    buffer.truncate(length);
    let directory = PathBuf::from(OsString::from_wide(&buffer));
    if !directory.is_absolute() {
        return Err(DeepSeekRuntimeError::new(
            "deepseek.system-directory-invalid",
            "The trusted Windows system directory is invalid.",
        ));
    }
    Ok(directory)
}

fn read_machine_node_installation() -> Result<PathBuf, DeepSeekRuntimeError> {
    let key = wide_null(NODE_REGISTRY_KEY);
    let value = wide_null(NODE_INSTALL_PATH_VALUE);
    let mut byte_count = 0u32;
    let sized = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            &mut byte_count,
        )
    };
    if sized != ERROR_SUCCESS
        || byte_count < size_of::<u16>() as u32
        || byte_count > MAX_REGISTRY_STRING_BYTES
        || !byte_count.is_multiple_of(size_of::<u16>() as u32)
    {
        return Err(DeepSeekRuntimeError::new(
            "deepseek.node-registration-unavailable",
            "The machine Node.js installation registration is unavailable.",
        ));
    }

    let mut buffer = vec![0u16; (byte_count as usize).div_ceil(size_of::<u16>())];
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut byte_count,
        )
    };
    if read != ERROR_SUCCESS {
        return Err(DeepSeekRuntimeError::new(
            "deepseek.node-registration-unavailable",
            "The machine Node.js installation registration is unavailable.",
        ));
    }
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    if end == 0 {
        return Err(DeepSeekRuntimeError::new(
            "deepseek.node-registration-invalid",
            "The machine Node.js installation registration is invalid.",
        ));
    }
    let installation = PathBuf::from(OsString::from_wide(&buffer[..end]));
    if !installation.is_absolute() {
        return Err(DeepSeekRuntimeError::new(
            "deepseek.node-registration-invalid",
            "The machine Node.js installation registration is invalid.",
        ));
    }
    Ok(installation)
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn validate_named_file(
    path: &Path,
    expected_name: &str,
    code: &'static str,
    message: &'static str,
) -> Result<(), DeepSeekRuntimeError> {
    if !path.is_absolute()
        || !path.is_file()
        || !path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.eq_ignore_ascii_case(expected_name))
    {
        return Err(DeepSeekRuntimeError::new(code, message));
    }
    Ok(())
}

pub(crate) struct DeepSeekRuntimeLifecycle {
    state: RuntimeState,
    parser: DeepSeekReadinessParser,
    ready_target: Option<SensitiveReadyTarget>,
    diagnostics: DiagnosticBuffer,
}

impl Default for DeepSeekRuntimeLifecycle {
    fn default() -> Self {
        Self {
            state: RuntimeState::default(),
            parser: DeepSeekReadinessParser::default(),
            ready_target: None,
            diagnostics: DiagnosticBuffer::new(
                DIAGNOSTIC_RECORDS,
                DIAGNOSTIC_TOTAL_BYTES,
                DIAGNOSTIC_RECORD_BYTES,
            ),
        }
    }
}

impl DeepSeekRuntimeLifecycle {
    pub(crate) fn phase(&self) -> RuntimePhase {
        self.state.phase()
    }

    fn take_ready_target(&mut self) -> Option<SensitiveReadyTarget> {
        self.ready_target.take()
    }

    pub(crate) fn begin_launch(&mut self) -> Result<(), DeepSeekRuntimeError> {
        self.state
            .transition_to(RuntimePhase::Starting)
            .map_err(|_| DeepSeekRuntimeError::invalid_transition())
    }

    pub(crate) fn record_owned_launch(
        &mut self,
        generation: u64,
    ) -> Result<(), DeepSeekRuntimeError> {
        self.state.claim_host_ownership(generation).map_err(|_| {
            DeepSeekRuntimeError::new(
                "deepseek.runtime-ownership-invalid",
                "Runtime ownership was not established by a valid host launch.",
            )
        })
    }

    pub(crate) fn ingest_stdout(
        &mut self,
        chunk: &[u8],
    ) -> Result<DeepSeekReadinessUpdate, DeepSeekRuntimeError> {
        if self.state.phase() != RuntimePhase::Starting && self.state.phase() != RuntimePhase::Ready
        {
            return Err(DeepSeekRuntimeError::new(
                "deepseek.readiness-state-invalid",
                "Readiness output arrived outside the starting runtime state.",
            ));
        }
        if !matches!(self.state.ownership(), RuntimeOwnership::HostOwned { .. }) {
            self.fail(
                "deepseek.readiness-unowned",
                "Readiness output was rejected because no owned runtime exists.",
            )?;
            return Ok(DeepSeekReadinessUpdate::Failed {
                code: "deepseek.readiness-unowned",
            });
        }

        let mut update = DeepSeekReadinessUpdate::Pending;
        for event in self.parser.push(chunk) {
            match event {
                Ok(target) => {
                    let port = target.port();
                    self.ready_target = Some(target);
                    if self.state.phase() == RuntimePhase::Starting {
                        self.state
                            .transition_to(RuntimePhase::Ready)
                            .map_err(|_| DeepSeekRuntimeError::invalid_transition())?;
                    }
                    update = DeepSeekReadinessUpdate::Ready { port };
                }
                Err(error) => {
                    let code = error.code();
                    self.fail(code, &error.to_string())?;
                    return Ok(DeepSeekReadinessUpdate::Failed { code });
                }
            }
        }
        Ok(update)
    }

    #[cfg(test)]
    pub(crate) fn process_exited_before_readiness(&mut self) -> Result<(), DeepSeekRuntimeError> {
        if self.state.phase() != RuntimePhase::Starting {
            return Err(DeepSeekRuntimeError::invalid_transition());
        }
        self.diagnostics.record(
            "deepseek.process-exited-before-readiness",
            "The owned process exited before readiness was established.",
        );
        self.ready_target = None;
        self.parser = DeepSeekReadinessParser::default();
        self.state.release_host_ownership();
        self.state
            .transition_to(RuntimePhase::Failed)
            .map_err(|_| DeepSeekRuntimeError::invalid_transition())
    }

    pub(crate) fn begin_stop(&mut self) -> Result<(), DeepSeekRuntimeError> {
        if !matches!(self.state.ownership(), RuntimeOwnership::HostOwned { .. }) {
            return Err(DeepSeekRuntimeError::new(
                "deepseek.stop-unowned",
                "Only a HarneSSHost-owned runtime can be stopped.",
            ));
        }
        self.state
            .transition_to(RuntimePhase::Stopping)
            .map_err(|_| DeepSeekRuntimeError::invalid_transition())?;
        self.ready_target = None;
        self.parser = DeepSeekReadinessParser::default();
        Ok(())
    }

    pub(crate) fn complete_stop(&mut self) -> Result<(), DeepSeekRuntimeError> {
        self.state
            .transition_to(RuntimePhase::Inactive)
            .map_err(|_| DeepSeekRuntimeError::invalid_transition())?;
        self.state.release_host_ownership();
        Ok(())
    }

    fn complete_failed_runtime_cleanup(&mut self) -> Result<(), DeepSeekRuntimeError> {
        if self.state.phase() != RuntimePhase::Failed {
            return Err(DeepSeekRuntimeError::invalid_transition());
        }
        self.state.release_host_ownership();
        Ok(())
    }

    pub(crate) fn record_sanitized_stderr(&mut self, source: &str) {
        self.diagnostics.record("deepseek.stderr", source);
    }

    fn fail(&mut self, code: &'static str, message: &str) -> Result<(), DeepSeekRuntimeError> {
        self.diagnostics.record(code, message);
        self.ready_target = None;
        self.parser = DeepSeekReadinessParser::default();
        self.state
            .transition_to(RuntimePhase::Failed)
            .map_err(|_| DeepSeekRuntimeError::invalid_transition())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeepSeekReadinessUpdate {
    Pending,
    Ready { port: NonZeroU16 },
    Failed { code: &'static str },
}

pub(crate) struct DeepSeekOwnedRuntime {
    process: OwnedWindowsRuntime,
    lifecycle: DeepSeekRuntimeLifecycle,
}

impl DeepSeekOwnedRuntime {
    pub(crate) fn launch(
        plan: DeepSeekLaunchPlan,
        generation: u64,
    ) -> Result<Self, DeepSeekRuntimeError> {
        Self::launch_with(plan, generation, launch_contained)
    }

    fn launch_with<F>(
        plan: DeepSeekLaunchPlan,
        generation: u64,
        launch: F,
    ) -> Result<Self, DeepSeekRuntimeError>
    where
        F: FnOnce(WindowsProcessSpec) -> Result<OwnedWindowsRuntime, WindowsSupervisorError>,
    {
        RuntimeState::validate_host_generation(generation).map_err(|_| {
            DeepSeekRuntimeError::new(
                "deepseek.runtime-ownership-invalid",
                "Runtime ownership was not established by a valid host launch.",
            )
        })?;
        let mut lifecycle = DeepSeekRuntimeLifecycle::default();
        lifecycle.begin_launch()?;
        let process = launch(plan.into_process_specification()).map_err(|error| {
            DeepSeekRuntimeError::from_windows(
                "deepseek.contained-launch-failed",
                "The verified DeepSeek launch could not be created inside its owned Job.",
                &error,
            )
        })?;
        lifecycle.record_owned_launch(generation)?;
        Ok(Self { process, lifecycle })
    }

    pub(crate) fn phase(&self) -> RuntimePhase {
        self.lifecycle.phase()
    }

    pub(crate) fn take_stdout(&mut self) -> Option<File> {
        self.process.take_stdout()
    }

    pub(crate) fn take_stderr(&mut self) -> Option<File> {
        self.process.take_stderr()
    }

    pub(crate) fn take_ready_target(&mut self) -> Option<SensitiveReadyTarget> {
        self.lifecycle.take_ready_target()
    }

    pub(crate) fn record_sanitized_stderr(&mut self, source: &str) {
        self.lifecycle.record_sanitized_stderr(source);
    }

    pub(crate) fn ingest_stdout(
        &mut self,
        chunk: &[u8],
    ) -> Result<DeepSeekReadinessUpdate, DeepSeekRuntimeError> {
        let update = self.lifecycle.ingest_stdout(chunk)?;
        if matches!(update, DeepSeekReadinessUpdate::Failed { .. }) {
            self.cleanup_failed_owned_runtime()?;
        }
        Ok(update)
    }

    pub(crate) fn fail_owned_runtime(
        &mut self,
        code: &'static str,
        message: &'static str,
    ) -> Result<(), DeepSeekRuntimeError> {
        self.lifecycle.fail(code, message)?;
        self.cleanup_failed_owned_runtime()
    }

    pub(crate) fn reconcile_unexpected_root_exit(&mut self) -> Result<(), DeepSeekRuntimeError> {
        let (code, message) = match self.lifecycle.phase() {
            RuntimePhase::Starting => (
                "deepseek.process-exited-before-readiness",
                "The owned process exited before readiness was established.",
            ),
            RuntimePhase::Ready => (
                "deepseek.runtime-exited-unexpectedly",
                "The owned DeepSeek runtime exited unexpectedly.",
            ),
            _ => return Err(DeepSeekRuntimeError::invalid_transition()),
        };
        self.fail_owned_runtime(code, message)
    }

    pub(crate) fn wait_for_root_exit(
        &self,
        timeout: Duration,
    ) -> Result<bool, DeepSeekRuntimeError> {
        self.process.wait_for_root_exit(timeout).map_err(|error| {
            DeepSeekRuntimeError::from_windows(
                "deepseek.root-observation-failed",
                "The owned DeepSeek root process could not be observed.",
                &error,
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn reconcile_exit_before_readiness(
        &mut self,
        timeout: Duration,
    ) -> Result<bool, DeepSeekRuntimeError> {
        if self.lifecycle.phase() != RuntimePhase::Starting {
            return Ok(false);
        }
        let empty = self.process.wait_for_empty(timeout).map_err(|error| {
            DeepSeekRuntimeError::from_windows(
                "deepseek.job-observation-failed",
                "The owned DeepSeek Job could not be observed.",
                &error,
            )
        })?;
        if empty {
            self.lifecycle.process_exited_before_readiness()?;
        }
        Ok(empty)
    }

    pub(crate) fn stop_owned(&mut self) -> Result<(), DeepSeekRuntimeError> {
        self.lifecycle.begin_stop()?;
        self.process.terminate_owned_job().map_err(|error| {
            DeepSeekRuntimeError::from_windows(
                "deepseek.owned-stop-failed",
                "The owned DeepSeek Job could not be terminated.",
                &error,
            )
        })?;
        if !self
            .process
            .wait_for_empty(OWNED_STOP_TIMEOUT)
            .map_err(|error| {
                DeepSeekRuntimeError::from_windows(
                    "deepseek.owned-stop-observation-failed",
                    "The owned DeepSeek Job could not be observed during shutdown.",
                    &error,
                )
            })?
        {
            return Err(DeepSeekRuntimeError::new(
                "deepseek.owned-stop-timeout",
                "The owned DeepSeek Job did not become inactive within the shutdown bound.",
            ));
        }
        self.lifecycle.complete_stop()
    }

    fn cleanup_failed_owned_runtime(&mut self) -> Result<(), DeepSeekRuntimeError> {
        self.process.terminate_owned_job().map_err(|error| {
            DeepSeekRuntimeError::from_windows(
                "deepseek.failed-runtime-cleanup-failed",
                "The failed owned DeepSeek Job could not be terminated.",
                &error,
            )
        })?;
        if !self
            .process
            .wait_for_empty(OWNED_STOP_TIMEOUT)
            .map_err(|error| {
                DeepSeekRuntimeError::from_windows(
                    "deepseek.failed-runtime-observation-failed",
                    "The failed owned DeepSeek Job could not be observed during cleanup.",
                    &error,
                )
            })?
        {
            return Err(DeepSeekRuntimeError::new(
                "deepseek.failed-runtime-cleanup-timeout",
                "The failed owned DeepSeek Job did not become inactive within the cleanup bound.",
            ));
        }
        self.lifecycle.complete_failed_runtime_cleanup()
    }
}

pub(crate) struct DeepSeekRuntimeError {
    code: &'static str,
    message: &'static str,
    windows_code: Option<&'static str>,
}

impl DeepSeekRuntimeError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            windows_code: None,
        }
    }

    fn from_windows(
        code: &'static str,
        message: &'static str,
        source: &WindowsSupervisorError,
    ) -> Self {
        Self {
            code,
            message,
            windows_code: Some(source.code()),
        }
    }

    fn invalid_transition() -> Self {
        Self::new(
            "deepseek.runtime-transition-invalid",
            "The DeepSeek runtime transition is not allowed.",
        )
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }
}

impl Debug for DeepSeekRuntimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeepSeekRuntimeError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("windows_code", &self.windows_code)
            .finish()
    }
}

impl Display for DeepSeekRuntimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self.windows_code {
            Some(windows_code) => write!(
                formatter,
                "{}: {} ({windows_code})",
                self.code, self.message
            ),
            None => write!(formatter, "{}: {}", self.code, self.message),
        }
    }
}

impl Error for DeepSeekRuntimeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::adapter::{CandidateSource, InstallationCandidate};
    use crate::harness::deepseek::{DeepSeekAdapter, TESTED_VERSION};
    use crate::library::build_harness_library;
    use crate::state::AppState;
    use std::io::Read;
    use std::sync::Arc;
    use tempfile::TempDir;

    const TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGH123456789";

    struct ToolFixture {
        _directory: TempDir,
        tools: ResolvedWindowsTools,
        command_processor: PathBuf,
        corepack_shim: PathBuf,
    }

    fn tool_fixture(corepack_parent: &str) -> ToolFixture {
        let directory = tempfile::tempdir().unwrap();
        let command_directory = directory.path().join("Windows System");
        let corepack_directory = directory.path().join(corepack_parent);
        std::fs::create_dir_all(&command_directory).unwrap();
        std::fs::create_dir_all(&corepack_directory).unwrap();
        let command_processor = command_directory.join(COMMAND_PROCESSOR_NAME);
        let corepack_shim = corepack_directory.join(COREPACK_SHIM_NAME);
        std::fs::write(&command_processor, b"").unwrap();
        std::fs::write(&corepack_shim, b"@echo off\r\n").unwrap();
        let tools =
            ResolvedWindowsTools::for_test(command_processor.clone(), corepack_shim.clone())
                .unwrap();
        ToolFixture {
            _directory: directory,
            tools,
            command_processor,
            corepack_shim,
        }
    }

    fn create_checkout(version: &str, directory_name: &str) -> (TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join(directory_name);
        let cli_source = root.join("apps").join("cli").join("src");
        std::fs::create_dir_all(&cli_source).unwrap();
        std::fs::write(cli_source.join("bin.ts"), "export {};\n").unwrap();
        std::fs::write(
            root.join("package.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": "@deepseek-ai/dsh-root",
                "version": version,
                "scripts": { "dsh": "node apps/cli/src/bin.ts" }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("apps").join("cli").join("package.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": "@deepseek-ai/dsh",
                "version": version,
                "bin": { "dsh": "lib/bin.js" }
            }))
            .unwrap(),
        )
        .unwrap();
        (directory, root)
    }

    fn registry() -> HarnessRegistry {
        let mut registry = HarnessRegistry::default();
        registry.register(Arc::new(DeepSeekAdapter::new())).unwrap();
        registry
    }

    fn context(path: &Path) -> DetectionContext {
        DetectionContext::new(vec![InstallationCandidate::new(
            HarnessId::new(DEEPSEEK_ADAPTER_ID),
            CandidateSource::DevelopmentCheckout,
            path.to_path_buf(),
        )])
    }

    fn command_line(plan: &DeepSeekLaunchPlan) -> String {
        match &plan.specification.command_line {
            WindowsCommandLine::TrustedPrequoted(units) => {
                OsString::from_wide(units).to_string_lossy().into_owned()
            }
            WindowsCommandLine::Arguments(_) => panic!("expected fixed command line"),
        }
    }

    fn verified_plan(
        checkout_path: &Path,
        tools: ResolvedWindowsTools,
    ) -> Result<DeepSeekLaunchPlan, DeepSeekRuntimeError> {
        prepare_deepseek_launch_with_tools(&registry(), &context(checkout_path), tools)
    }

    fn plan_error(
        result: Result<DeepSeekLaunchPlan, DeepSeekRuntimeError>,
    ) -> DeepSeekRuntimeError {
        match result {
            Ok(_) => panic!("expected launch plan to be blocked"),
            Err(error) => error,
        }
    }

    fn ready_line() -> String {
        format!("dsh web: http://127.0.0.1:43127/?token={TOKEN}\n")
    }

    #[test]
    fn gate_4b3_verified_installation_builds_fixed_launch_plan() {
        let (_checkout, root) = create_checkout(TESTED_VERSION, "verified checkout & not shell");
        let fixture = tool_fixture("Program Files\\nodejs");
        let expected_command_processor = fixture.command_processor.clone();
        let expected_corepack = fixture.corepack_shim.clone();
        let plan = verified_plan(&root, fixture.tools).unwrap();

        assert!(plan.specification.executable.is_absolute());
        assert_eq!(plan.specification.executable, expected_command_processor);
        assert_eq!(
            plan.specification.working_directory.as_deref(),
            Some(std::fs::canonicalize(&root).unwrap().as_path())
        );
        let rendered = command_line(&plan);
        let expected = format!(
            "\"{}\" /d /s /v:off /c \"\"{}\" pnpm dsh web --no-open --host 127.0.0.1 --port 0\"",
            expected_command_processor.display(),
            expected_corepack.display()
        );
        assert_eq!(rendered, expected);
        assert!(!rendered.contains(root.to_string_lossy().as_ref()));
    }

    #[test]
    fn gate_4b3_missing_invalid_and_unsupported_installations_are_blocked() {
        let missing_tools = tool_fixture("Program Files\\nodejs");
        let missing = plan_error(prepare_deepseek_launch_with_tools(
            &registry(),
            &DetectionContext::default(),
            missing_tools.tools,
        ));
        assert_eq!(missing.code(), "deepseek.launch-installation-not-found");

        let (_invalid_owner, invalid_root) = create_checkout(TESTED_VERSION, "invalid");
        std::fs::write(invalid_root.join("package.json"), b"{}").unwrap();
        let invalid_tools = tool_fixture("Program Files\\nodejs");
        let invalid = plan_error(verified_plan(&invalid_root, invalid_tools.tools));
        assert_eq!(invalid.code(), "deepseek.launch-installation-invalid");

        let (_unsupported_owner, unsupported_root) = create_checkout("0.1.3", "unsupported");
        let unsupported_tools = tool_fixture("Program Files\\nodejs");
        let unsupported = plan_error(verified_plan(&unsupported_root, unsupported_tools.tools));
        assert_eq!(unsupported.code(), "deepseek.launch-version-unsupported");
    }

    #[test]
    fn gate_4b3_corepack_is_explicitly_selected_and_shell_metacharacters_are_rejected() {
        let (_checkout, root) = create_checkout(TESTED_VERSION, "verified");
        let explicit_tools = tool_fixture("Program Files\\nodejs");
        let explicit_corepack = explicit_tools.corepack_shim.clone();
        let plan = verified_plan(&root, explicit_tools.tools).unwrap();
        let rendered = command_line(&plan);
        assert!(rendered.contains(explicit_corepack.to_string_lossy().as_ref()));
        assert!(!rendered.contains(" /c corepack "));

        let unsafe_tools = tool_fixture("node&tools");
        let error = plan_error(verified_plan(&root, unsafe_tools.tools));
        assert_eq!(error.code(), "deepseek.corepack-shim-path-unsafe");
    }

    #[test]
    #[ignore = "requires machine Node.js/Corepack registration and the accepted development checkout"]
    fn gate_4b3_real_machine_discovery_integration() {
        let discovered = ResolvedWindowsTools::discover().unwrap();
        assert!(discovered.command_processor.is_absolute());
        assert!(discovered.corepack_shim.is_absolute());
        let state = AppState::new().unwrap();
        let live_plan = prepare_deepseek_launch(&state.registry, &state.detection_context).unwrap();
        assert!(live_plan.specification.executable.is_absolute());
        assert!(live_plan
            .specification
            .working_directory
            .as_ref()
            .is_some_and(|directory| directory.is_absolute()));
    }

    #[test]
    fn gate_4b3_zero_generation_is_rejected_before_the_process_boundary() {
        use std::cell::Cell;

        let (_checkout, root) = create_checkout(TESTED_VERSION, "zero generation");
        let fixture = tool_fixture("Program Files\\nodejs");
        let plan = verified_plan(&root, fixture.tools).unwrap();
        let process_boundary_reached = Cell::new(false);

        let error = match DeepSeekOwnedRuntime::launch_with(plan, 0, |_| {
            process_boundary_reached.set(true);
            unreachable!("zero generation must fail before process creation")
        }) {
            Ok(_) => panic!("zero generation must be rejected"),
            Err(error) => error,
        };

        assert_eq!(error.code(), "deepseek.runtime-ownership-invalid");
        assert!(!process_boundary_reached.get());
    }

    #[test]
    fn gate_4b3_valid_stdout_reaches_ready_without_serializing_the_token() {
        let mut lifecycle = DeepSeekRuntimeLifecycle::default();
        lifecycle.begin_launch().unwrap();
        lifecycle.record_owned_launch(1).unwrap();

        let update = lifecycle.ingest_stdout(ready_line().as_bytes()).unwrap();

        assert_eq!(
            update,
            DeepSeekReadinessUpdate::Ready {
                port: NonZeroU16::new(43127).unwrap()
            }
        );
        assert_eq!(lifecycle.phase(), RuntimePhase::Ready);
        assert!(!format!("{update:?}").contains(TOKEN));
        assert!(!format!("{:?}", lifecycle.diagnostics.records()).contains(TOKEN));

        let library =
            serde_json::to_string(&build_harness_library(&AppState::new().unwrap())).unwrap();
        assert!(!library.contains(TOKEN));
        assert!(!library.contains("?token="));
    }

    #[test]
    fn gate_4b3_malformed_readiness_and_pre_ready_exit_fail_safely() {
        let mut malformed = DeepSeekRuntimeLifecycle::default();
        malformed.begin_launch().unwrap();
        malformed.record_owned_launch(2).unwrap();
        let update = malformed
            .ingest_stdout(b"dsh web: http://localhost:1/?token=secret\n")
            .unwrap();
        assert_eq!(
            update,
            DeepSeekReadinessUpdate::Failed {
                code: "deepseek.readiness-malformed"
            }
        );
        assert_eq!(malformed.phase(), RuntimePhase::Failed);
        assert!(!format!("{:?}", malformed.diagnostics.records()).contains("secret"));

        malformed
            .record_sanitized_stderr(&format!("failed URL http://127.0.0.1:43127/?token={TOKEN}"));
        assert!(!format!("{:?}", malformed.diagnostics.records()).contains(TOKEN));
        assert!(format!("{:?}", malformed.diagnostics.records()).contains("[REDACTED]"));
        malformed.complete_failed_runtime_cleanup().unwrap();
        assert_eq!(malformed.state.ownership(), RuntimeOwnership::Unowned);

        let mut exited = DeepSeekRuntimeLifecycle::default();
        exited.begin_launch().unwrap();
        exited.record_owned_launch(3).unwrap();
        exited.process_exited_before_readiness().unwrap();
        assert_eq!(exited.phase(), RuntimePhase::Failed);
        assert_eq!(exited.state.ownership(), RuntimeOwnership::Unowned);
    }

    #[test]
    fn gate_4b3_foreign_readiness_never_infers_ownership() {
        let mut lifecycle = DeepSeekRuntimeLifecycle::default();
        lifecycle.begin_launch().unwrap();

        let update = lifecycle.ingest_stdout(ready_line().as_bytes()).unwrap();

        assert_eq!(
            update,
            DeepSeekReadinessUpdate::Failed {
                code: "deepseek.readiness-unowned"
            }
        );
        assert_eq!(lifecycle.phase(), RuntimePhase::Failed);
        assert_eq!(lifecycle.state.ownership(), RuntimeOwnership::Unowned);
    }

    #[test]
    fn gate_4b3_owned_start_ready_stop_transitions_are_preserved() {
        let mut lifecycle = DeepSeekRuntimeLifecycle::default();
        lifecycle.begin_launch().unwrap();
        assert_eq!(lifecycle.phase(), RuntimePhase::Starting);
        lifecycle.record_owned_launch(4).unwrap();
        lifecycle.ingest_stdout(ready_line().as_bytes()).unwrap();
        assert_eq!(lifecycle.phase(), RuntimePhase::Ready);

        lifecycle.begin_stop().unwrap();
        assert_eq!(lifecycle.phase(), RuntimePhase::Stopping);
        lifecycle.complete_stop().unwrap();
        assert_eq!(lifecycle.phase(), RuntimePhase::Inactive);
        assert_eq!(lifecycle.state.ownership(), RuntimeOwnership::Unowned);
    }

    #[test]
    fn gate_4b3_synthetic_cmd_wrapper_uses_containment_and_readiness_pipeline() {
        let (_checkout, root) = create_checkout(TESTED_VERSION, "synthetic checkout");
        let fixture = tool_fixture("Program Files\\nodejs");
        std::fs::write(
            &fixture.corepack_shim,
            format!(
                "@echo off\r\necho cwd=%CD%\r\necho args=%*\r\necho dsh web: http://127.0.0.1:43127/?token={TOKEN}\r\n"
            ),
        )
        .unwrap();
        let system_command_processor = resolve_system_directory()
            .unwrap()
            .join(COMMAND_PROCESSOR_NAME);
        let tools =
            ResolvedWindowsTools::for_test(system_command_processor, fixture.corepack_shim.clone())
                .unwrap();
        let plan = verified_plan(&root, tools).unwrap();

        let mut runtime = DeepSeekOwnedRuntime::launch(plan, 5).unwrap();
        let mut stdout = runtime.take_stdout().unwrap();
        assert!(runtime.wait_for_root_exit(Duration::from_secs(5)).unwrap());
        let mut output = Vec::new();
        stdout.read_to_end(&mut output).unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("args=pnpm dsh web --no-open --host 127.0.0.1 --port 0"));
        let reported_directory = rendered
            .lines()
            .find_map(|line| line.strip_prefix("cwd="))
            .expect("synthetic wrapper should report its working directory");
        assert_eq!(
            std::fs::canonicalize(reported_directory).unwrap(),
            std::fs::canonicalize(&root).unwrap()
        );
        assert_eq!(
            runtime.ingest_stdout(rendered.as_bytes()).unwrap(),
            DeepSeekReadinessUpdate::Ready {
                port: NonZeroU16::new(43127).unwrap()
            }
        );
        assert_eq!(runtime.phase(), RuntimePhase::Ready);
        assert!(runtime.take_stderr().is_some());
        runtime.reconcile_unexpected_root_exit().unwrap();
        assert_eq!(runtime.phase(), RuntimePhase::Failed);
        assert_eq!(
            runtime.lifecycle.state.ownership(),
            RuntimeOwnership::Unowned
        );
        assert_eq!(runtime.process.active_process_count().unwrap(), 0);
    }

    #[test]
    fn gate_4b3_synthetic_process_exit_before_readiness_reaches_failed() {
        let (_checkout, root) = create_checkout(TESTED_VERSION, "synthetic failed checkout");
        let fixture = tool_fixture("Program Files\\nodejs");
        std::fs::write(&fixture.corepack_shim, "@echo off\r\nexit /b 7\r\n").unwrap();
        let system_command_processor = resolve_system_directory()
            .unwrap()
            .join(COMMAND_PROCESSOR_NAME);
        let tools =
            ResolvedWindowsTools::for_test(system_command_processor, fixture.corepack_shim.clone())
                .unwrap();
        let plan = verified_plan(&root, tools).unwrap();

        let mut runtime = DeepSeekOwnedRuntime::launch(plan, 6).unwrap();
        assert!(runtime.wait_for_root_exit(Duration::from_secs(5)).unwrap());
        assert!(runtime
            .reconcile_exit_before_readiness(Duration::from_secs(5))
            .unwrap());
        assert_eq!(runtime.phase(), RuntimePhase::Failed);
    }
}
