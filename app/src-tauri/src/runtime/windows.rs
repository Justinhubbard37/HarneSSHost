use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::mem::{forget, size_of};
use std::os::windows::ffi::OsStrExt;
#[cfg(test)]
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
#[cfg(test)]
use windows_sys::Win32::System::JobObjects::JobObjectBasicProcessIdList;
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, IsProcessInJob, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, EXTENDED_STARTUPINFO_PRESENT,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

const ATTRIBUTE_COUNT: u32 = 2;
const MAX_ARGUMENTS: usize = 32;
const MAX_COMMAND_LINE_UNITS: usize = 32_767;
const JOB_TERMINATION_EXIT_CODE: u32 = 0x4848_0001;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
pub(super) const CONTAINED_PROCESS_CREATION_FLAGS: u32 =
    EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW;

pub(crate) struct WindowsProcessSpec {
    executable: PathBuf,
    command_line: WindowsCommandLine,
    working_directory: Option<PathBuf>,
}

enum WindowsCommandLine {
    Arguments(Vec<OsString>),
    TrustedPrequoted(Vec<u16>),
}

impl WindowsProcessSpec {
    pub(crate) fn new(executable: PathBuf, arguments: Vec<OsString>) -> Self {
        Self {
            executable,
            command_line: WindowsCommandLine::Arguments(arguments),
            working_directory: None,
        }
    }

    pub(crate) fn with_working_directory(mut self, working_directory: PathBuf) -> Self {
        self.working_directory = Some(working_directory);
        self
    }

    pub(crate) fn with_trusted_prequoted_command_line(mut self, command_line: Vec<u16>) -> Self {
        self.command_line = WindowsCommandLine::TrustedPrequoted(command_line);
        self
    }

    #[cfg(test)]
    pub(crate) fn trusted_prequoted_command_line(&self) -> Option<&[u16]> {
        match &self.command_line {
            WindowsCommandLine::TrustedPrequoted(command_line) => Some(command_line),
            WindowsCommandLine::Arguments(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn executable(&self) -> &Path {
        &self.executable
    }

    #[cfg(test)]
    pub(crate) fn working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    fn validate(&self) -> Result<(), WindowsSupervisorError> {
        if !self.executable.is_absolute() {
            return Err(WindowsSupervisorError::new(
                "windows.spec-executable-not-absolute",
                "The process executable must use an absolute path.",
            ));
        }
        if let WindowsCommandLine::Arguments(arguments) = &self.command_line {
            if arguments.len() > MAX_ARGUMENTS {
                return Err(WindowsSupervisorError::new(
                    "windows.spec-too-many-arguments",
                    "The process specification contains too many arguments.",
                ));
            }
        }
        if self
            .working_directory
            .as_ref()
            .is_some_and(|directory| !directory.is_absolute())
        {
            return Err(WindowsSupervisorError::new(
                "windows.spec-working-directory-not-absolute",
                "The process working directory must use an absolute path.",
            ));
        }
        Ok(())
    }

    fn command_line(&self) -> Result<Vec<u16>, WindowsSupervisorError> {
        match &self.command_line {
            WindowsCommandLine::Arguments(arguments) => {
                build_command_line(self.executable.as_os_str(), arguments)
            }
            WindowsCommandLine::TrustedPrequoted(command_line) => {
                if command_line.contains(&0) {
                    return Err(WindowsSupervisorError::new(
                        "windows.spec-interior-null",
                        "The process specification contains an invalid null character.",
                    ));
                }
                if command_line.len() + 1 > MAX_COMMAND_LINE_UNITS {
                    return Err(WindowsSupervisorError::new(
                        "windows.spec-command-line-too-long",
                        "The process specification exceeds the Windows command-line limit.",
                    ));
                }
                let mut terminated = command_line.clone();
                terminated.push(0);
                Ok(terminated)
            }
        }
    }
}

pub(crate) struct OwnedWindowsRuntime {
    job: OwnedHandle,
    root_process: OwnedHandle,
    stdout: Option<File>,
    stderr: Option<File>,
}

impl OwnedWindowsRuntime {
    pub(crate) fn active_process_count(&self) -> Result<u32, WindowsSupervisorError> {
        query_active_process_count(self.job.as_raw())
    }

    #[cfg(test)]
    pub(crate) fn root_is_in_owned_job(&self) -> Result<bool, WindowsSupervisorError> {
        process_is_in_job(self.root_process.as_raw(), self.job.as_raw())
    }

    #[cfg(test)]
    pub(crate) fn job_limit_flags(&self) -> Result<u32, WindowsSupervisorError> {
        query_job_limit_flags(self.job.as_raw())
    }

    pub(crate) fn terminate_owned_job(&self) -> Result<(), WindowsSupervisorError> {
        let result = unsafe { TerminateJobObject(self.job.as_raw(), JOB_TERMINATION_EXIT_CODE) };
        if result == 0 {
            return Err(WindowsSupervisorError::last_os_error(
                "windows.terminate-job-failed",
                "The owned Job could not be terminated.",
            ));
        }
        Ok(())
    }

    pub(crate) fn wait_for_empty(&self, timeout: Duration) -> Result<bool, WindowsSupervisorError> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.active_process_count()? == 0 {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    pub(crate) fn wait_for_root_exit(
        &self,
        timeout: Duration,
    ) -> Result<bool, WindowsSupervisorError> {
        match unsafe { WaitForSingleObject(self.root_process.as_raw(), duration_millis(timeout)) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            WAIT_FAILED => Err(WindowsSupervisorError::last_os_error(
                "windows.wait-root-failed",
                "The root process wait failed.",
            )),
            _ => Err(WindowsSupervisorError::new(
                "windows.wait-root-unexpected",
                "The root process wait returned an unexpected state.",
            )),
        }
    }

    pub(crate) fn take_stdout(&mut self) -> Option<File> {
        self.stdout.take()
    }

    pub(crate) fn take_stderr(&mut self) -> Option<File> {
        self.stderr.take()
    }

    #[cfg(test)]
    pub(crate) fn retained_handles_are_noninheritable_for_test(
        &self,
    ) -> Result<(), WindowsSupervisorError> {
        ensure_not_inheritable(self.job.as_raw(), "windows.test-job-handle-inheritable")?;
        if let Some(stdout) = &self.stdout {
            ensure_not_inheritable(
                stdout.as_raw_handle() as HANDLE,
                "windows.test-stdout-read-handle-inheritable",
            )?;
        }
        if let Some(stderr) = &self.stderr {
            ensure_not_inheritable(
                stderr.as_raw_handle() as HANDLE,
                "windows.test-stderr-read-handle-inheritable",
            )?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn process_ids_for_test(&self) -> Result<Vec<u32>, WindowsSupervisorError> {
        query_process_ids_for_test(self.job.as_raw())
    }
}

pub(crate) fn launch_contained(
    specification: WindowsProcessSpec,
) -> Result<OwnedWindowsRuntime, WindowsSupervisorError> {
    launch_with_hook(specification, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LaunchCheckpoint {
    BeforeJobCreation,
    BeforeJobConfiguration,
    AfterStdoutPipeCreation,
    AfterPipeCreation,
    BeforeNullOpen,
    AfterNullOpen,
    AfterAttributeAllocation,
    AfterAttributeInitialization,
    AfterHandleListUpdate,
    AfterAttributeUpdates,
    AfterCreateProcess,
}

#[cfg(test)]
pub(crate) fn launch_with_test_hook<F>(
    specification: WindowsProcessSpec,
    hook: F,
) -> Result<OwnedWindowsRuntime, WindowsSupervisorError>
where
    F: FnMut(LaunchCheckpoint) -> Result<(), WindowsSupervisorError>,
{
    launch_with_hook(specification, hook)
}

fn launch_with_hook<F>(
    specification: WindowsProcessSpec,
    mut checkpoint: F,
) -> Result<OwnedWindowsRuntime, WindowsSupervisorError>
where
    F: FnMut(LaunchCheckpoint) -> Result<(), WindowsSupervisorError>,
{
    specification.validate()?;
    let application_name = wide_null(specification.executable.as_os_str())?;
    let mut command_line = specification.command_line()?;
    let working_directory = specification
        .working_directory
        .as_deref()
        .map(wide_current_directory)
        .transpose()?;
    let working_directory_pointer = working_directory
        .as_ref()
        .map_or(null(), |directory| directory.as_ptr());

    checkpoint(LaunchCheckpoint::BeforeJobCreation)?;
    let job = create_private_job()?;
    checkpoint(LaunchCheckpoint::BeforeJobConfiguration)?;
    configure_kill_on_close(job.as_raw())?;

    let stdout_pipe = AnonymousPipe::create()?;
    checkpoint(LaunchCheckpoint::AfterStdoutPipeCreation)?;
    let stderr_pipe = AnonymousPipe::create()?;
    checkpoint(LaunchCheckpoint::AfterPipeCreation)?;

    checkpoint(LaunchCheckpoint::BeforeNullOpen)?;
    let child_stdin = open_inheritable_null()?;
    checkpoint(LaunchCheckpoint::AfterNullOpen)?;

    ensure_not_inheritable(job.as_raw(), "windows.job-handle-inheritable")?;
    ensure_not_inheritable(
        stdout_pipe.read.as_raw(),
        "windows.stdout-read-handle-inheritable",
    )?;
    ensure_not_inheritable(
        stderr_pipe.read.as_raw(),
        "windows.stderr-read-handle-inheritable",
    )?;
    ensure_inheritable(child_stdin.as_raw(), "windows.stdin-handle-not-inheritable")?;
    ensure_inheritable(
        stdout_pipe.write.as_raw(),
        "windows.stdout-write-handle-not-inheritable",
    )?;
    ensure_inheritable(
        stderr_pipe.write.as_raw(),
        "windows.stderr-write-handle-not-inheritable",
    )?;

    let mut attributes = ProcThreadAttributeList::allocate(ATTRIBUTE_COUNT)?;
    checkpoint(LaunchCheckpoint::AfterAttributeAllocation)?;
    attributes.initialize(ATTRIBUTE_COUNT)?;
    checkpoint(LaunchCheckpoint::AfterAttributeInitialization)?;

    let inherited_handles = [
        child_stdin.as_raw(),
        stdout_pipe.write.as_raw(),
        stderr_pipe.write.as_raw(),
    ];
    attributes.update(
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
        inherited_handles.as_ptr().cast(),
        size_of_val(&inherited_handles),
        "windows.handle-list-update-failed",
        "The restricted inherited-handle list could not be configured.",
    )?;
    checkpoint(LaunchCheckpoint::AfterHandleListUpdate)?;

    let job_list = [job.as_raw()];
    attributes.update(
        PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
        job_list.as_ptr().cast(),
        size_of_val(&job_list),
        "windows.job-list-update-failed",
        "The atomic Job assignment list could not be configured.",
    )?;
    checkpoint(LaunchCheckpoint::AfterAttributeUpdates)?;

    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = child_stdin.as_raw();
    startup.StartupInfo.hStdOutput = stdout_pipe.write.as_raw();
    startup.StartupInfo.hStdError = stderr_pipe.write.as_raw();
    startup.lpAttributeList = attributes.as_raw();

    let mut process_information = PROCESS_INFORMATION::default();
    let created = unsafe {
        CreateProcessW(
            application_name.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CONTAINED_PROCESS_CREATION_FLAGS,
            null(),
            working_directory_pointer,
            &startup.StartupInfo,
            &mut process_information,
        )
    };
    if created == 0 {
        return Err(WindowsSupervisorError::last_os_error(
            "windows.create-process-failed",
            "The contained process could not be created.",
        ));
    }

    let root_process = OwnedHandle::from_created(
        process_information.hProcess,
        "windows.process-handle-invalid",
        "CreateProcessW returned an invalid process handle.",
    )?;
    let initial_thread = OwnedHandle::from_created(
        process_information.hThread,
        "windows.thread-handle-invalid",
        "CreateProcessW returned an invalid initial-thread handle.",
    )?;
    drop(initial_thread);

    if !process_is_in_job(root_process.as_raw(), job.as_raw())? {
        return Err(WindowsSupervisorError::new(
            "windows.atomic-containment-missing",
            "The created process was not atomically associated with its owned Job.",
        ));
    }
    checkpoint(LaunchCheckpoint::AfterCreateProcess)?;

    drop(attributes);
    drop(child_stdin);
    drop(stdout_pipe.write);
    drop(stderr_pipe.write);

    Ok(OwnedWindowsRuntime {
        job,
        root_process,
        stdout: Some(stdout_pipe.read.into_file()),
        stderr: Some(stderr_pipe.read.into_file()),
    })
}

struct AnonymousPipe {
    read: OwnedHandle,
    write: OwnedHandle,
}

impl AnonymousPipe {
    fn create() -> Result<Self, WindowsSupervisorError> {
        let security = inheritable_security_attributes();
        let mut raw_read = null_mut();
        let mut raw_write = null_mut();
        let created = unsafe { CreatePipe(&mut raw_read, &mut raw_write, &security, 0) };
        if created == 0 {
            return Err(WindowsSupervisorError::last_os_error(
                "windows.pipe-creation-failed",
                "An anonymous output pipe could not be created.",
            ));
        }

        let read = OwnedHandle::from_created(
            raw_read,
            "windows.pipe-read-handle-invalid",
            "CreatePipe returned an invalid read handle.",
        )?;
        let write = OwnedHandle::from_created(
            raw_write,
            "windows.pipe-write-handle-invalid",
            "CreatePipe returned an invalid write handle.",
        )?;
        set_inheritability(read.as_raw(), false)?;
        ensure_not_inheritable(read.as_raw(), "windows.pipe-read-handle-inheritable")?;
        ensure_inheritable(write.as_raw(), "windows.pipe-write-handle-not-inheritable")?;

        Ok(Self { read, write })
    }
}

struct OwnedHandle {
    raw: HANDLE,
}

impl OwnedHandle {
    fn from_created(
        raw: HANDLE,
        code: &'static str,
        message: &'static str,
    ) -> Result<Self, WindowsSupervisorError> {
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            return Err(WindowsSupervisorError::new(code, message));
        }
        Ok(Self { raw })
    }

    fn as_raw(&self) -> HANDLE {
        self.raw
    }

    fn into_file(self) -> File {
        let raw = self.raw;
        forget(self);
        unsafe { File::from_raw_handle(raw as RawHandle) }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() && self.raw != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.raw);
            }
        }
    }
}

struct ProcThreadAttributeList {
    _storage: Vec<usize>,
    raw: LPPROC_THREAD_ATTRIBUTE_LIST,
    initialized: bool,
}

impl ProcThreadAttributeList {
    fn allocate(attribute_count: u32) -> Result<Self, WindowsSupervisorError> {
        let mut required_bytes = 0usize;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), attribute_count, 0, &mut required_bytes);
        }
        if required_bytes == 0 {
            return Err(WindowsSupervisorError::last_os_error(
                "windows.attribute-size-query-failed",
                "The process attribute-list size could not be determined.",
            ));
        }

        let word_size = size_of::<usize>();
        let word_count = required_bytes.div_ceil(word_size);
        let mut storage = vec![0usize; word_count];
        let raw = storage.as_mut_ptr().cast();
        Ok(Self {
            _storage: storage,
            raw,
            initialized: false,
        })
    }

    fn initialize(&mut self, attribute_count: u32) -> Result<(), WindowsSupervisorError> {
        let mut bytes = self._storage.len() * size_of::<usize>();
        let initialized =
            unsafe { InitializeProcThreadAttributeList(self.raw, attribute_count, 0, &mut bytes) };
        if initialized == 0 {
            return Err(WindowsSupervisorError::last_os_error(
                "windows.attribute-initialization-failed",
                "The process attribute list could not be initialized.",
            ));
        }
        self.initialized = true;
        Ok(())
    }

    fn update(
        &mut self,
        attribute: usize,
        value: *const std::ffi::c_void,
        value_bytes: usize,
        error_code: &'static str,
        error_message: &'static str,
    ) -> Result<(), WindowsSupervisorError> {
        let updated = unsafe {
            UpdateProcThreadAttribute(
                self.raw,
                0,
                attribute,
                value,
                value_bytes,
                null_mut(),
                null(),
            )
        };
        if updated == 0 {
            return Err(WindowsSupervisorError::last_os_error(
                error_code,
                error_message,
            ));
        }
        Ok(())
    }

    fn as_raw(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.raw
    }
}

impl Drop for ProcThreadAttributeList {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.raw);
            }
        }
    }
}

fn create_private_job() -> Result<OwnedHandle, WindowsSupervisorError> {
    let raw = unsafe { CreateJobObjectW(null(), null()) };
    let job = OwnedHandle::from_created(
        raw,
        "windows.job-creation-failed",
        "A private anonymous Job could not be created.",
    )?;
    ensure_not_inheritable(job.as_raw(), "windows.job-handle-inheritable")?;
    Ok(job)
}

fn configure_kill_on_close(job: HANDLE) -> Result<(), WindowsSupervisorError> {
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        return Err(WindowsSupervisorError::last_os_error(
            "windows.job-configuration-failed",
            "The private Job could not be configured for kill-on-close.",
        ));
    }
    Ok(())
}

fn query_active_process_count(job: HANDLE) -> Result<u32, WindowsSupervisorError> {
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    let queried = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    };
    if queried == 0 {
        return Err(WindowsSupervisorError::last_os_error(
            "windows.job-accounting-query-failed",
            "The owned Job process count could not be queried.",
        ));
    }
    Ok(accounting.ActiveProcesses)
}

#[cfg(test)]
fn query_job_limit_flags(job: HANDLE) -> Result<u32, WindowsSupervisorError> {
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let queried = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            null_mut(),
        )
    };
    if queried == 0 {
        return Err(WindowsSupervisorError::last_os_error(
            "windows.job-limit-query-failed",
            "The owned Job limits could not be queried.",
        ));
    }
    Ok(limits.BasicLimitInformation.LimitFlags)
}

#[cfg(test)]
fn query_process_ids_for_test(job: HANDLE) -> Result<Vec<u32>, WindowsSupervisorError> {
    const MAX_TEST_PROCESSES: usize = 16;

    #[repr(C)]
    struct ProcessIdBuffer {
        assigned: u32,
        present: u32,
        process_ids: [usize; MAX_TEST_PROCESSES],
    }

    let mut buffer = ProcessIdBuffer {
        assigned: 0,
        present: 0,
        process_ids: [0; MAX_TEST_PROCESSES],
    };
    let queried = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicProcessIdList,
            (&mut buffer as *mut ProcessIdBuffer).cast(),
            size_of::<ProcessIdBuffer>() as u32,
            null_mut(),
        )
    };
    if queried == 0 {
        return Err(WindowsSupervisorError::last_os_error(
            "windows.test-job-process-list-query-failed",
            "The test-only owned Job process list could not be queried.",
        ));
    }
    if buffer.assigned as usize > MAX_TEST_PROCESSES || buffer.present as usize > MAX_TEST_PROCESSES
    {
        return Err(WindowsSupervisorError::new(
            "windows.test-job-process-list-overflow",
            "The test-only owned Job process list exceeded its fixture bound.",
        ));
    }

    buffer.process_ids[..buffer.present as usize]
        .iter()
        .copied()
        .map(|process_id| {
            u32::try_from(process_id).map_err(|_| {
                WindowsSupervisorError::new(
                    "windows.test-process-id-out-of-range",
                    "A test-only process identifier was out of range.",
                )
            })
        })
        .collect()
}

fn process_is_in_job(process: HANDLE, job: HANDLE) -> Result<bool, WindowsSupervisorError> {
    let mut result = 0;
    let queried = unsafe { IsProcessInJob(process, job, &mut result) };
    if queried == 0 {
        return Err(WindowsSupervisorError::last_os_error(
            "windows.job-membership-query-failed",
            "Process Job membership could not be queried.",
        ));
    }
    Ok(result != 0)
}

fn open_inheritable_null() -> Result<OwnedHandle, WindowsSupervisorError> {
    const NULL_DEVICE: [u16; 4] = [b'N' as u16, b'U' as u16, b'L' as u16, 0];
    let security = inheritable_security_attributes();
    let raw = unsafe {
        CreateFileW(
            NULL_DEVICE.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &security,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    let handle = OwnedHandle::from_created(
        raw,
        "windows.null-open-failed",
        "The controlled NUL input handle could not be opened.",
    )?;
    ensure_inheritable(handle.as_raw(), "windows.null-handle-not-inheritable")?;
    Ok(handle)
}

fn inheritable_security_attributes() -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    }
}

fn set_inheritability(handle: HANDLE, inheritable: bool) -> Result<(), WindowsSupervisorError> {
    let flags = if inheritable { HANDLE_FLAG_INHERIT } else { 0 };
    let updated = unsafe {
        windows_sys::Win32::Foundation::SetHandleInformation(handle, HANDLE_FLAG_INHERIT, flags)
    };
    if updated == 0 {
        return Err(WindowsSupervisorError::last_os_error(
            "windows.handle-inheritability-update-failed",
            "Handle inheritance could not be restricted.",
        ));
    }
    Ok(())
}

fn ensure_not_inheritable(
    handle: HANDLE,
    error_code: &'static str,
) -> Result<(), WindowsSupervisorError> {
    ensure_inheritability(handle, false, error_code)
}

fn ensure_inheritable(
    handle: HANDLE,
    error_code: &'static str,
) -> Result<(), WindowsSupervisorError> {
    ensure_inheritability(handle, true, error_code)
}

fn ensure_inheritability(
    handle: HANDLE,
    expected: bool,
    error_code: &'static str,
) -> Result<(), WindowsSupervisorError> {
    let mut flags = 0;
    let queried = unsafe { GetHandleInformation(handle, &mut flags) };
    if queried == 0 {
        return Err(WindowsSupervisorError::last_os_error(
            "windows.handle-flags-query-failed",
            "Handle inheritance flags could not be queried.",
        ));
    }
    let actual = flags & HANDLE_FLAG_INHERIT != 0;
    if actual != expected {
        return Err(WindowsSupervisorError::new(
            error_code,
            "A Windows handle violated the restricted inheritance contract.",
        ));
    }
    Ok(())
}

fn wide_null(value: &OsStr) -> Result<Vec<u16>, WindowsSupervisorError> {
    let mut wide = value.encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(WindowsSupervisorError::new(
            "windows.spec-interior-null",
            "The process specification contains an invalid null character.",
        ));
    }
    wide.push(0);
    Ok(wide)
}

fn wide_current_directory(path: &Path) -> Result<Vec<u16>, WindowsSupervisorError> {
    const VERBATIM_PREFIX: [u16; 4] = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const VERBATIM_UNC_PREFIX: [u16; 8] = [
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];

    let encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let normalized = if encoded.starts_with(&VERBATIM_UNC_PREFIX) {
        let mut ordinary_unc = vec![b'\\' as u16, b'\\' as u16];
        ordinary_unc.extend_from_slice(&encoded[VERBATIM_UNC_PREFIX.len()..]);
        ordinary_unc
    } else if encoded.starts_with(&VERBATIM_PREFIX) {
        encoded[VERBATIM_PREFIX.len()..].to_vec()
    } else {
        encoded
    };
    if normalized.contains(&0) {
        return Err(WindowsSupervisorError::new(
            "windows.spec-interior-null",
            "The process specification contains an invalid null character.",
        ));
    }
    let mut terminated = normalized;
    terminated.push(0);
    Ok(terminated)
}

fn build_command_line(
    executable: &OsStr,
    arguments: &[OsString],
) -> Result<Vec<u16>, WindowsSupervisorError> {
    let mut command_line = Vec::new();
    append_quoted_argument(&mut command_line, executable)?;
    for argument in arguments {
        command_line.push(b' ' as u16);
        append_quoted_argument(&mut command_line, argument)?;
    }
    if command_line.len() + 1 > MAX_COMMAND_LINE_UNITS {
        return Err(WindowsSupervisorError::new(
            "windows.spec-command-line-too-long",
            "The process specification exceeds the Windows command-line limit.",
        ));
    }
    command_line.push(0);
    Ok(command_line)
}

pub(crate) fn append_quoted_argument(
    command_line: &mut Vec<u16>,
    argument: &OsStr,
) -> Result<(), WindowsSupervisorError> {
    let units = argument.encode_wide().collect::<Vec<_>>();
    if units.contains(&0) {
        return Err(WindowsSupervisorError::new(
            "windows.spec-interior-null",
            "The process specification contains an invalid null character.",
        ));
    }
    let requires_quotes =
        units.is_empty() || units.iter().any(|unit| matches!(*unit, 0x20 | 0x09 | 0x22));
    if !requires_quotes {
        command_line.extend_from_slice(&units);
        return Ok(());
    }

    command_line.push(b'"' as u16);
    let mut backslashes = 0usize;
    for unit in units {
        if unit == b'\\' as u16 {
            backslashes += 1;
            continue;
        }
        if unit == b'"' as u16 {
            command_line.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2 + 1));
            command_line.push(unit);
            backslashes = 0;
            continue;
        }
        command_line.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
        backslashes = 0;
        command_line.push(unit);
    }
    command_line.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
    command_line.push(b'"' as u16);
    Ok(())
}

fn duration_millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX - 1)
}

pub(crate) struct WindowsSupervisorError {
    code: &'static str,
    message: &'static str,
    os_code: Option<i32>,
}

impl WindowsSupervisorError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            os_code: None,
        }
    }

    fn last_os_error(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            os_code: std::io::Error::last_os_error().raw_os_error(),
        }
    }

    #[cfg(test)]
    pub(crate) fn injected(code: &'static str) -> Self {
        Self::new(code, "A test-only launch failure was injected.")
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }
}

impl std::fmt::Debug for WindowsSupervisorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsSupervisorError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("os_code", &self.os_code)
            .finish()
    }
}

impl Display for WindowsSupervisorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self.os_code {
            Some(os_code) => write!(
                formatter,
                "{}: {} (Windows error {os_code})",
                self.code, self.message
            ),
            None => write!(formatter, "{}: {}", self.code, self.message),
        }
    }
}

impl Error for WindowsSupervisorError {}
