use super::windows::{
    launch_contained, launch_with_test_hook, LaunchCheckpoint, OwnedWindowsRuntime,
    WindowsProcessSpec, WindowsSupervisorError, CONTAINED_PROCESS_CREATION_FLAGS,
};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessHandleCount, OpenProcess, TerminateProcess, WaitForSingleObject,
    CREATE_NO_WINDOW, EXTENDED_STARTUPINFO_PRESENT, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE,
};

const FIXTURE_DIRECTORY_ENV: &str = "HARNESS_HOST_4B2_FIXTURE_DIRECTORY";
const FIXTURE_WAIT: Duration = Duration::from_secs(15);
const PROCESS_EXIT_WAIT: Duration = Duration::from_secs(10);
const FIXTURE_SLEEP: Duration = Duration::from_secs(60);
const OWNER_CRASH_EXIT_CODE: u32 = 0x4848_0002;
const FIXTURE_ROOT: &str = "runtime::windows_tests::fixture_root";
const FIXTURE_ROOT_EXIT: &str = "runtime::windows_tests::fixture_root_exit";
const FIXTURE_DESCENDANT: &str = "runtime::windows_tests::fixture_descendant";
const FIXTURE_GRANDCHILD: &str = "runtime::windows_tests::fixture_grandchild";
const FIXTURE_FOREIGN: &str = "runtime::windows_tests::fixture_foreign";
const FIXTURE_OWNER_CRASH: &str = "runtime::windows_tests::fixture_owner_crash";
static WINDOWS_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn windows_creation_flags_are_extended_and_windowless() {
    assert_eq!(
        CONTAINED_PROCESS_CREATION_FLAGS,
        EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW
    );
}

struct FixtureContext {
    _environment: EnvironmentGuard,
    directory: TempDir,
    // Declared last so the process-wide fixture environment is restored before the mutex unlocks.
    _lock: MutexGuard<'static, ()>,
}

impl FixtureContext {
    fn new() -> Self {
        let lock = WINDOWS_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = tempfile::tempdir().expect("fixture directory should be created");
        let environment = EnvironmentGuard::set(
            FIXTURE_DIRECTORY_ENV,
            directory.path().as_os_str().to_os_string(),
        );
        Self {
            _environment: environment,
            directory,
            _lock: lock,
        }
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn launch(&self, fixture: &str) -> OwnedWindowsRuntime {
        launch_contained(fixture_spec(fixture)).expect("fixture should launch inside an owned Job")
    }

    fn wait_for_tree(&self) -> BTreeMap<&'static str, u32> {
        for marker in ["root.ready", "descendant.ready", "grandchild.ready"] {
            wait_for_file(&self.path().join(marker), FIXTURE_WAIT);
        }
        BTreeMap::from([
            ("root", read_process_id(&self.path().join("root.pid"))),
            (
                "descendant",
                read_process_id(&self.path().join("descendant.pid")),
            ),
            (
                "grandchild",
                read_process_id(&self.path().join("grandchild.pid")),
            ),
        ])
    }
}

struct EnvironmentGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvironmentGuard {
    fn set(key: &'static str, value: OsString) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var(self.key, previous),
            None => std::env::remove_var(self.key),
        }
    }
}

struct ChildGuard(Child);

impl ChildGuard {
    fn spawn(fixture: &str, directory: &Path) -> Self {
        let child = Command::new(current_test_executable())
            .args(fixture_arguments(fixture))
            .env(FIXTURE_DIRECTORY_ENV, directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("external fixture should start");
        Self(child)
    }

    fn is_running(&mut self) -> bool {
        self.0
            .try_wait()
            .expect("external fixture state should be observable")
            .is_none()
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !self.is_running() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        !self.is_running()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn windows_integration_atomic_containment_and_restricted_handles() {
    let context = FixtureContext::new();
    let runtime = context.launch(FIXTURE_ROOT);
    let processes = context.wait_for_tree();

    assert!(runtime
        .root_is_in_owned_job()
        .expect("root Job membership should be queryable"));
    assert_eq!(
        runtime
            .job_limit_flags()
            .expect("Job limits should be queryable"),
        windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    );
    assert_job_contains(&runtime, processes.values().copied());
    runtime
        .retained_handles_are_noninheritable_for_test()
        .expect("retained parent and Job handles must be non-inheritable");

    let source = include_str!("windows.rs");
    let forbidden_api = ["AssignProcessTo", "JobObject"].concat();
    assert!(!source.contains(&forbidden_api));
    assert!(source.contains("PROC_THREAD_ATTRIBUTE_JOB_LIST"));
    assert!(source.contains("PROC_THREAD_ATTRIBUTE_HANDLE_LIST"));
    assert!(source.contains("EXTENDED_STARTUPINFO_PRESENT"));

    runtime
        .terminate_owned_job()
        .expect("owned Job termination should succeed");
    assert!(runtime
        .wait_for_empty(PROCESS_EXIT_WAIT)
        .expect("owned Job should remain queryable"));
}

#[test]
fn windows_integration_descendants_outlive_root_handle_boundary() {
    let context = FixtureContext::new();
    let runtime = context.launch(FIXTURE_ROOT_EXIT);
    wait_for_file(&context.path().join("root.ready"), FIXTURE_WAIT);
    wait_for_file(&context.path().join("descendant.ready"), FIXTURE_WAIT);
    wait_for_file(&context.path().join("grandchild.ready"), FIXTURE_WAIT);

    assert!(runtime
        .wait_for_root_exit(FIXTURE_WAIT)
        .expect("root exit should be observable"));
    let descendant = read_process_id(&context.path().join("descendant.pid"));
    let grandchild = read_process_id(&context.path().join("grandchild.pid"));
    assert_job_contains(&runtime, [descendant, grandchild]);
    assert!(
        runtime
            .active_process_count()
            .expect("Job process count should be queryable")
            >= 2
    );

    runtime
        .terminate_owned_job()
        .expect("owned Job termination should succeed");
    assert!(runtime
        .wait_for_empty(PROCESS_EXIT_WAIT)
        .expect("owned Job should remain queryable"));
}

#[test]
fn windows_integration_output_capture_nul_stdin_and_pipe_eof() {
    let context = FixtureContext::new();
    let mut runtime = context.launch(FIXTURE_ROOT);
    let stdout = runtime.take_stdout().expect("stdout reader should exist");
    let stderr = runtime.take_stderr().expect("stderr reader should exist");
    let (stdout_sender, stdout_receiver) = mpsc::channel();
    let (stderr_sender, stderr_receiver) = mpsc::channel();
    thread::spawn(move || stdout_sender.send(read_to_end(stdout)).ok());
    thread::spawn(move || stderr_sender.send(read_to_end(stderr)).ok());

    context.wait_for_tree();
    runtime
        .terminate_owned_job()
        .expect("owned Job termination should succeed");
    assert!(runtime
        .wait_for_empty(PROCESS_EXIT_WAIT)
        .expect("owned Job should remain queryable"));

    let stdout = stdout_receiver
        .recv_timeout(PROCESS_EXIT_WAIT)
        .expect("stdout should reach EOF");
    let stderr = stderr_receiver
        .recv_timeout(PROCESS_EXIT_WAIT)
        .expect("stderr should reach EOF");
    assert!(stdout.contains("fixture-root-stdout"));
    assert!(stdout.contains("fixture-descendant-stdout"));
    assert!(stdout.contains("fixture-grandchild-stdout"));
    assert!(stdout.contains("fixture-stdin-eof=0"));
    assert!(stderr.contains("fixture-root-stderr"));
    assert!(stderr.contains("fixture-descendant-stderr"));
    assert!(stderr.contains("fixture-grandchild-stderr"));
}

#[test]
fn windows_integration_owned_tree_termination_preserves_foreign_process() {
    let context = FixtureContext::new();
    let mut foreign = ChildGuard::spawn(FIXTURE_FOREIGN, context.path());
    wait_for_file(&context.path().join("foreign.ready"), FIXTURE_WAIT);
    let runtime = context.launch(FIXTURE_ROOT);
    let fixture_processes = context.wait_for_tree();

    runtime
        .terminate_owned_job()
        .expect("owned Job termination should succeed");
    assert!(runtime
        .wait_for_empty(PROCESS_EXIT_WAIT)
        .expect("owned Job should remain queryable"));
    for process_id in fixture_processes.values().copied() {
        assert!(wait_for_process_exit(process_id, PROCESS_EXIT_WAIT));
    }
    assert!(foreign.is_running());
}

#[test]
fn windows_integration_kill_on_final_job_handle_close() {
    let context = FixtureContext::new();
    let runtime = context.launch(FIXTURE_ROOT);
    let fixture_processes = context.wait_for_tree();

    drop(runtime);

    for process_id in fixture_processes.values().copied() {
        assert!(
            wait_for_process_exit(process_id, PROCESS_EXIT_WAIT),
            "fixture process {process_id} survived final Job handle closure"
        );
    }
}

#[test]
fn windows_integration_owner_crash_closes_job_and_tree() {
    let context = FixtureContext::new();
    let mut owner = ChildGuard::spawn(FIXTURE_OWNER_CRASH, context.path());
    wait_for_file(&context.path().join("owner.ready"), FIXTURE_WAIT);
    let fixture_processes = context.wait_for_tree();

    assert!(owner.wait_for_exit(PROCESS_EXIT_WAIT));
    for process_id in fixture_processes.values().copied() {
        assert!(
            wait_for_process_exit(process_id, PROCESS_EXIT_WAIT),
            "fixture process {process_id} survived abrupt owner termination"
        );
    }
}

#[test]
fn windows_integration_failure_paths_release_handles_and_processes() {
    let context = FixtureContext::new();

    let warmup = context.launch(FIXTURE_ROOT);
    context.wait_for_tree();
    warmup
        .terminate_owned_job()
        .expect("warm-up Job termination should succeed");
    assert!(warmup
        .wait_for_empty(PROCESS_EXIT_WAIT)
        .expect("warm-up Job should empty"));
    drop(warmup);

    let missing = context.path().join("missing-fixture.exe");
    let warm_failure = match launch_contained(WindowsProcessSpec::new(missing.clone(), Vec::new()))
    {
        Ok(_) => panic!("CreateProcessW warm-up failure should be returned"),
        Err(error) => error,
    };
    assert_eq!(warm_failure.code(), "windows.create-process-failed");
    let baseline_handles = current_process_handle_count();

    for target in [
        LaunchCheckpoint::BeforeJobCreation,
        LaunchCheckpoint::BeforeJobConfiguration,
        LaunchCheckpoint::AfterStdoutPipeCreation,
        LaunchCheckpoint::AfterPipeCreation,
        LaunchCheckpoint::BeforeNullOpen,
        LaunchCheckpoint::AfterNullOpen,
        LaunchCheckpoint::AfterAttributeAllocation,
        LaunchCheckpoint::AfterAttributeInitialization,
        LaunchCheckpoint::AfterHandleListUpdate,
        LaunchCheckpoint::AfterAttributeUpdates,
        LaunchCheckpoint::AfterCreateProcess,
    ] {
        let result = launch_with_test_hook(fixture_spec(FIXTURE_ROOT), |checkpoint| {
            if checkpoint == target {
                Err(WindowsSupervisorError::injected(
                    "windows.test-injected-failure",
                ))
            } else {
                Ok(())
            }
        });
        let error = match result {
            Ok(_) => panic!("injected launch failure should be returned"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "windows.test-injected-failure");
        wait_for_handle_count(
            baseline_handles,
            PROCESS_EXIT_WAIT,
            &format!("injected checkpoint {target:?}"),
        );
    }

    let error = match launch_contained(WindowsProcessSpec::new(missing, Vec::new())) {
        Ok(_) => panic!("CreateProcessW failure should be returned"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "windows.create-process-failed");
    wait_for_handle_count(
        baseline_handles,
        PROCESS_EXIT_WAIT,
        "CreateProcessW failure",
    );
}

#[test]
#[ignore = "disposable Gate 4B-2 subprocess fixture"]
fn fixture_root() {
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .expect("controlled stdin should be readable");
    assert!(input.is_empty());
    println!("fixture-root-stdout");
    println!("fixture-stdin-eof={}", input.len());
    eprintln!("fixture-root-stderr");
    write_current_process_id("root.pid");
    let descendant = spawn_fixture(FIXTURE_DESCENDANT);
    write_marker("root.ready");
    thread::sleep(FIXTURE_SLEEP);
    drop(descendant);
}

#[test]
#[ignore = "disposable Gate 4B-2 subprocess fixture"]
fn fixture_root_exit() {
    write_current_process_id("root.pid");
    let descendant = spawn_fixture(FIXTURE_DESCENDANT);
    write_marker("root.ready");
    drop(descendant);
}

#[test]
#[ignore = "disposable Gate 4B-2 subprocess fixture"]
fn fixture_descendant() {
    println!("fixture-descendant-stdout");
    eprintln!("fixture-descendant-stderr");
    write_current_process_id("descendant.pid");
    let grandchild = spawn_fixture(FIXTURE_GRANDCHILD);
    write_marker("descendant.ready");
    thread::sleep(FIXTURE_SLEEP);
    drop(grandchild);
}

#[test]
#[ignore = "disposable Gate 4B-2 subprocess fixture"]
fn fixture_grandchild() {
    println!("fixture-grandchild-stdout");
    eprintln!("fixture-grandchild-stderr");
    write_current_process_id("grandchild.pid");
    write_marker("grandchild.ready");
    thread::sleep(FIXTURE_SLEEP);
}

#[test]
#[ignore = "disposable Gate 4B-2 subprocess fixture"]
fn fixture_foreign() {
    write_current_process_id("foreign.pid");
    write_marker("foreign.ready");
    thread::sleep(FIXTURE_SLEEP);
}

#[test]
#[ignore = "disposable Gate 4B-2 subprocess fixture"]
fn fixture_owner_crash() {
    let directory = fixture_directory();
    let runtime = launch_contained(fixture_spec(FIXTURE_ROOT))
        .expect("crash-owner fixture should create its private Job");
    wait_for_file(&directory.join("root.ready"), FIXTURE_WAIT);
    wait_for_file(&directory.join("descendant.ready"), FIXTURE_WAIT);
    wait_for_file(&directory.join("grandchild.ready"), FIXTURE_WAIT);
    assert!(
        runtime
            .active_process_count()
            .expect("crash-owner Job should be queryable")
            >= 3
    );
    write_marker("owner.ready");

    unsafe {
        TerminateProcess(GetCurrentProcess(), OWNER_CRASH_EXIT_CODE);
    }
    unreachable!("TerminateProcess should end the disposable owner fixture");
}

fn fixture_spec(fixture: &str) -> WindowsProcessSpec {
    WindowsProcessSpec::new(current_test_executable(), fixture_arguments(fixture))
}

fn current_test_executable() -> PathBuf {
    std::env::current_exe().expect("current test executable should be available")
}

fn fixture_arguments(fixture: &str) -> Vec<OsString> {
    ["--ignored", "--exact", fixture, "--nocapture"]
        .into_iter()
        .map(OsString::from)
        .collect()
}

fn spawn_fixture(fixture: &str) -> Child {
    Command::new(current_test_executable())
        .args(fixture_arguments(fixture))
        .spawn()
        .expect("descendant fixture should start")
}

fn fixture_directory() -> PathBuf {
    std::env::var_os(FIXTURE_DIRECTORY_ENV)
        .map(PathBuf::from)
        .expect("fixture directory should be configured")
}

fn write_marker(name: &str) {
    let path = fixture_directory().join(name);
    let mut file = File::create(path).expect("fixture marker should be created");
    file.write_all(b"ready")
        .expect("fixture marker should be written");
    file.sync_all().expect("fixture marker should be flushed");
}

fn write_current_process_id(name: &str) {
    fs::write(
        fixture_directory().join(name),
        std::process::id().to_string(),
    )
    .expect("fixture process identifier should be written");
}

fn read_process_id(path: &Path) -> u32 {
    fs::read_to_string(path)
        .expect("fixture process identifier should be readable")
        .parse()
        .expect("fixture process identifier should be numeric")
}

fn wait_for_file(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.is_file() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("fixture marker did not appear: {}", path.display());
}

fn assert_job_contains(
    runtime: &OwnedWindowsRuntime,
    expected_process_ids: impl IntoIterator<Item = u32>,
) {
    let actual = runtime
        .process_ids_for_test()
        .expect("owned Job process identifiers should be queryable");
    for expected in expected_process_ids {
        assert!(
            actual.contains(&expected),
            "owned Job did not contain fixture process {expected}; actual: {actual:?}"
        );
    }
}

fn read_to_end(mut file: File) -> String {
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .expect("captured pipe should be readable through EOF");
    String::from_utf8(bytes).expect("fixture output should be UTF-8")
}

fn wait_for_process_exit(process_id: u32, timeout: Duration) -> bool {
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            process_id,
        )
    };
    if process.is_null() {
        return true;
    }
    let result = unsafe { WaitForSingleObject(process, duration_millis(timeout)) };
    unsafe {
        CloseHandle(process);
    }
    match result {
        WAIT_OBJECT_0 => true,
        WAIT_TIMEOUT => false,
        WAIT_FAILED => panic!("fixture process wait failed"),
        other => panic!("unexpected fixture process wait state: {other}"),
    }
}

fn current_process_handle_count() -> u32 {
    let mut count = 0;
    let queried = unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
    assert_ne!(
        queried, 0,
        "current-process handle count should be queryable"
    );
    count
}

fn wait_for_handle_count(expected: u32, timeout: Duration, context: &str) {
    let deadline = Instant::now() + timeout;
    loop {
        let actual = current_process_handle_count();
        if actual == expected {
            return;
        }
        if Instant::now() >= deadline {
            panic!(
                "handle count did not return to baseline after {context}: expected {expected}, actual {actual}"
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn duration_millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX - 1)
}
