//! Kernel-enforced limits for the decode worker.
//!
//! On macOS the worker gets its isolation from a signed XPC service with its
//! own App Sandbox, and the broker only supervises memory: the README states
//! plainly that worker memory is supervised rather than hard-capped. This
//! module is the Windows row of §5.5, where the kernel applies the limits
//! rather than the application: LPAC without capabilities, a Job Object,
//! an enumerated set of inherited handles and a token with privileges dropped.
//! Each launch owns an ephemeral profile and read/execute-only code copy.
//! No ACL grants are made on photographs or the application data directory.
//! The worker verifies LPAC, capabilities and Job limits before external input.
//! Native measurements and limitations belong to ADR 0009.
//!
//! Off Windows the type exists and grants nothing. `kernel_enforced` is the
//! honest answer to what a build actually has, so provenance and reports can
//! state it instead of implying an isolation that is not there.
// `Context` is only reached by the portable branches below.
#[cfg(not(windows))]
use anyhow::Context;
#[cfg(windows)]
mod appcontainer;
use anyhow::Result;
use std::{
    io::{Read, Write},
    path::Path,
    process::Child,
};

/// The worker process, however this platform manages to start it.
///
/// The broker only ever needs its two pipes and the ability to end it, so the
/// difference between a confined process and an ordinary child stays here.
pub struct Worker(WorkerKind);

enum WorkerKind {
    #[cfg(not(windows))]
    Child {
        child: Child,
        stdin: Option<std::process::ChildStdin>,
        stdout: Option<std::process::ChildStdout>,
    },
    #[cfg(windows)]
    Confined(windows::Confined),
}

impl Worker {
    /// The write and read ends, taken once. A second call is a caller error
    /// and fails rather than handing out a second reference to the same pipe.
    pub fn take_pipes(&mut self) -> Result<(Box<dyn Write + Send>, Box<dyn Read + Send>)> {
        match &mut self.0 {
            #[cfg(not(windows))]
            WorkerKind::Child { stdin, stdout, .. } => Ok((
                Box::new(stdin.take().context("stdin worker già preso")?),
                Box::new(stdout.take().context("stdout worker già preso")?),
            )),
            #[cfg(windows)]
            WorkerKind::Confined(confined) => confined.take_pipes(),
        }
    }

    /// End the worker and reap it. Both steps are best effort: a process that
    /// already exited must not turn shutdown into an error.
    pub fn terminate(&mut self) {
        match &mut self.0 {
            #[cfg(not(windows))]
            WorkerKind::Child { child, .. } => {
                let _ = child.kill();
                let _ = child.wait();
            }
            #[cfg(windows)]
            WorkerKind::Confined(confined) => confined.terminate(),
        }
    }

    /// Whether the process has already exited. `None` means still running.
    pub fn exited(&mut self) -> Option<i32> {
        match &mut self.0 {
            #[cfg(not(windows))]
            WorkerKind::Child { child, .. } => child
                .try_wait()
                .ok()
                .flatten()
                .map(|s| s.code().unwrap_or(-1)),
            #[cfg(windows)]
            WorkerKind::Confined(confined) => confined.exited(),
        }
    }
}

pub struct Isolation {
    #[cfg(windows)]
    job: windows::Job,
    /// Byte ceiling asked of the kernel; zero when nothing was requested.
    limit: u64,
}

impl Isolation {
    /// No limits beyond those the operating system already applies. Used by
    /// the XPC path, where the service owns its own sandbox.
    pub fn none() -> Self {
        Self {
            #[cfg(windows)]
            job: windows::Job::inert(),
            limit: 0,
        }
    }

    /// Ask the kernel for a committed-memory ceiling, a single live process
    /// and termination of the whole group when this value is dropped.
    ///
    /// A platform without an implementation returns a value that grants
    /// nothing rather than an error: the worker still runs under the broker's
    /// own credit system, and `kernel_enforced` reports the difference.
    pub fn bounded(memory_bytes: u64) -> Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                job: windows::Job::bounded(memory_bytes)?,
                limit: memory_bytes,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = memory_bytes;
            Ok(Self::none())
        }
    }

    /// Place an already started child under these limits.
    ///
    /// Kept for a process this type did not start. `spawn` is the confined
    /// path: it leaves no window in which the worker runs unbounded.
    pub fn adopt(&self, child: &Child) -> Result<()> {
        #[cfg(windows)]
        {
            self.job.adopt(child)
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            Ok(())
        }
    }

    /// Start the worker inside LPAC and Job limits, with a filtered environment
    /// and its own read/execute-only code directory.
    ///
    /// On Windows the process is created into the job and under a restricted
    /// token in one call, so there is no interval in which it runs unbounded
    /// and no handle beyond its three pipes is inherited. Elsewhere this is an
    /// ordinary child process and `kernel_enforced` says so.
    pub fn spawn(&self, binary: &Path, working_directory: &Path) -> Result<Worker> {
        #[cfg(windows)]
        {
            Ok(Worker(WorkerKind::Confined(
                self.job.spawn(binary, working_directory)?,
            )))
        }
        #[cfg(not(windows))]
        {
            let mut child = std::process::Command::new(binary)
                .env_clear()
                .current_dir(working_directory)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .context("Avvio tr-worker; compilare entrambi i binari")?;
            self.adopt(&child)?;
            let stdin = child.stdin.take().context("stdin worker")?;
            let stdout = child.stdout.take().context("stdout worker")?;
            Ok(Worker(WorkerKind::Child {
                child,
                stdin: Some(stdin),
                stdout: Some(stdout),
            }))
        }
    }

    /// True only where the ceiling is applied by the kernel. The broker's own
    /// admission credits are not an answer to this question.
    pub fn kernel_enforced(&self) -> bool {
        cfg!(windows) && self.limit > 0
    }

    /// Whether this worker is actually inside these limits, asked of the
    /// kernel rather than inferred from having called the right function.
    ///
    /// Off Windows there is nothing to confine to and the answer is `false`,
    /// which is what `kernel_enforced` already reports.
    pub fn confines(&self, worker: &Worker) -> bool {
        #[cfg(windows)]
        {
            let WorkerKind::Confined(confined) = &worker.0;
            self.job.confines(confined)
        }
        #[cfg(not(windows))]
        {
            let _ = worker;
            false
        }
    }

    /// Ceiling actually requested, for provenance and reports.
    pub fn limit_bytes(&self) -> u64 {
        self.limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A build with no implementation must say so. Reporting confinement it
    /// does not have would be worse than reporting none.
    #[test]
    fn confinement_is_claimed_only_where_the_kernel_applies_it() {
        assert!(!Isolation::none().kernel_enforced());
        let bounded = Isolation::bounded(64 * 1024 * 1024).unwrap();
        assert_eq!(bounded.kernel_enforced(), cfg!(windows));
        assert_eq!(
            bounded.limit_bytes(),
            if cfg!(windows) { 64 * 1024 * 1024 } else { 0 }
        );
    }

    /// A worker started by `spawn` must already be inside the job when the
    /// broker first sees it, with no interval in which it ran unbounded. The
    /// kernel answers that question, not the fact of having called `spawn`.
    #[cfg(windows)]
    #[test]
    fn the_worker_is_inside_the_job_from_creation() {
        let binary = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/debug/tr-worker.exe");
        if !binary.exists() {
            return; // Built by scripts/verify.sh; nothing to assert without it.
        }
        let isolation = Isolation::bounded(384 * 1024 * 1024).unwrap();
        let mut worker = isolation.spawn(&binary, &std::env::temp_dir()).unwrap();
        assert!(
            isolation.confines(&worker),
            "il worker non risulta nel job subito dopo la creazione"
        );
        assert!(worker.exited().is_none(), "atteso in attesa sulla pipe");
        // A different job must not claim it: confinement is to this one.
        let other = Isolation::bounded(384 * 1024 * 1024).unwrap();
        assert!(!other.confines(&worker));
        worker.terminate();
    }

    /// The limits must be the kernel's, not the broker's bookkeeping. Two
    /// facts prove it: a second process is refused because the job caps the
    /// live count, and closing the job ends what is still inside it.
    #[cfg(windows)]
    #[test]
    fn the_job_caps_live_processes_and_ends_them_when_closed() {
        use std::{process::Command, thread::sleep, time::Duration};
        // `ping` directly, never through `cmd`: a shell would start a second
        // process inside a job that admits one, and the job would empty itself
        // before the assertions ran.
        let spawn = || {
            Command::new("ping")
                .args(["-n", "30", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("processo di prova")
        };
        let isolation = Isolation::bounded(64 * 1024 * 1024).unwrap();
        let mut first = spawn();
        isolation.adopt(&first).expect("primo processo confinato");
        let mut second = spawn();
        assert!(
            isolation.adopt(&second).is_err(),
            "il limite di processi attivi non è applicato dal kernel"
        );
        let _ = second.kill();
        let _ = second.wait();

        assert!(
            first.try_wait().unwrap().is_none(),
            "vivo prima di chiudere"
        );
        drop(isolation);
        // Termination is asynchronous; the loop bounds the wait rather than
        // asserting on a single sample.
        let mut ended = None;
        for _ in 0..50 {
            if let Some(status) = first.try_wait().unwrap() {
                ended = Some(status);
                break;
            }
            sleep(Duration::from_millis(20));
        }
        assert!(
            ended.is_some(),
            "chiudere il job non ha terminato il processo confinato"
        );
    }
}

#[cfg(windows)]
mod windows {
    use anyhow::{Context, Result, bail};
    use std::{
        ffi::OsStr,
        fs::File,
        io::{Read, Write},
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        path::Path,
        process::Child,
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, STILL_ACTIVE, SetHandleInformation,
        },
        Security::{
            AllocateAndInitializeSid, CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, FreeSid,
            SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES, SECURITY_NT_AUTHORITY, SID_AND_ATTRIBUTES,
            TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY,
        },
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
                JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
                JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JobObjectExtendedLimitInformation, SetInformationJobObject,
            },
            Pipes::CreatePipe,
            Threading::{
                CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
                DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
                GetExitCodeProcess, InitializeProcThreadAttributeList,
                LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcessToken, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
                PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
                TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
            },
        },
    };

    /// Owns a job handle. Dropping it terminates every process still inside,
    /// so an abandoned worker cannot outlive the broker that started it.
    pub struct Job(Option<HANDLE>);
    // The handle is owned exclusively and only passed to kernel calls.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn inert() -> Self {
            Self(None)
        }

        pub fn bounded(memory_bytes: u64) -> Result<Self> {
            // An anonymous job: nothing else on the machine can open it by name.
            let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if handle.is_null() {
                bail!(
                    "CreateJobObject fallita: {}",
                    std::io::Error::last_os_error()
                );
            }
            let job = Self(Some(handle));
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_JOB_MEMORY
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
            // One decoder per job: the worker has no reason to spawn helpers,
            // and a job that allows them would not bound what it can start.
            limits.BasicLimitInformation.ActiveProcessLimit = 1;
            let ceiling = usize::try_from(memory_bytes).unwrap_or(usize::MAX);
            limits.ProcessMemoryLimit = ceiling;
            limits.JobMemoryLimit = ceiling;
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                bail!(
                    "SetInformationJobObject fallita: {}",
                    std::io::Error::last_os_error()
                );
            }
            Ok(job)
        }

        /// Asks the kernel whether the process is inside this job. A worker
        /// created through `spawn` is inside it from its first instruction.
        pub fn confines(&self, confined: &Confined) -> bool {
            let (Some(job), Some(process)) = (self.0, confined.process.as_ref()) else {
                return false;
            };
            let mut inside = 0;
            let asked = unsafe { IsProcessInJob(process.as_raw_handle(), job, &mut inside) };
            asked != 0 && inside != 0
        }

        pub fn adopt(&self, child: &Child) -> Result<()> {
            let Some(handle) = self.0 else {
                return Ok(());
            };
            let ok = unsafe { AssignProcessToJobObject(handle, child.as_raw_handle() as HANDLE) };
            if ok == 0 {
                bail!(
                    "AssignProcessToJobObject fallita: {}",
                    std::io::Error::last_os_error()
                );
            }
            Ok(())
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            if let Some(handle) = self.0.take() {
                // Kill-on-close is a limit flag, so this also ends the worker.
                unsafe { CloseHandle(handle) };
            }
        }
    }

    fn failed(call: &str) -> anyhow::Error {
        anyhow::anyhow!("{call} fallita: {}", std::io::Error::last_os_error())
    }

    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(Some(0)).collect()
    }

    /// One end of a pipe, plus the end handed to the child.
    struct Pair {
        ours: OwnedHandle,
        theirs: OwnedHandle,
    }

    /// `child_reads` selects which end the child keeps. Only that end stays
    /// inheritable; ours is cleared, so the child never receives a handle to
    /// the side the broker holds.
    fn pipe(child_reads: bool) -> Result<Pair> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 1,
        };
        let (mut read, mut write) = (ptr::null_mut(), ptr::null_mut());
        if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
            return Err(failed("CreatePipe"));
        }
        // SAFETY: both handles were just produced by CreatePipe and are owned.
        let (read, write) = unsafe {
            (
                OwnedHandle::from_raw_handle(read),
                OwnedHandle::from_raw_handle(write),
            )
        };
        let (theirs, ours) = if child_reads {
            (read, write)
        } else {
            (write, read)
        };
        if unsafe { SetHandleInformation(ours.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(failed("SetHandleInformation"));
        }
        Ok(Pair { ours, theirs })
    }

    /// The caller's own token with every privilege dropped and the local
    /// administrators group marked deny-only.
    ///
    /// Because it is a restriction of the caller's token, `CreateProcessAsUser`
    /// accepts it without the privileges that call normally demands. Tighter
    /// steps, restricting SIDs and a low integrity level, change what the
    /// worker can load and each needs its own measurement before being added.
    fn restricted_token() -> Result<OwnedHandle> {
        let mut token = ptr::null_mut();
        let access = TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY;
        if unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) } == 0 {
            return Err(failed("OpenProcessToken"));
        }
        // SAFETY: OpenProcessToken succeeded, so the handle is owned.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };

        let mut administrators = ptr::null_mut();
        let authority = SECURITY_NT_AUTHORITY;
        // BUILTIN domain (32) and the Administrators alias (544).
        if unsafe {
            AllocateAndInitializeSid(
                &authority,
                2,
                32,
                544,
                0,
                0,
                0,
                0,
                0,
                0,
                &mut administrators,
            )
        } == 0
        {
            return Err(failed("AllocateAndInitializeSid"));
        }
        let deny = [SID_AND_ATTRIBUTES {
            Sid: administrators,
            Attributes: 0,
        }];
        let mut restricted = ptr::null_mut();
        let ok = unsafe {
            CreateRestrictedToken(
                token.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE,
                deny.len() as u32,
                deny.as_ptr(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                &mut restricted,
            )
        };
        unsafe { FreeSid(administrators) };
        if ok == 0 {
            return Err(failed("CreateRestrictedToken"));
        }
        // SAFETY: CreateRestrictedToken succeeded, so the handle is owned.
        Ok(unsafe { OwnedHandle::from_raw_handle(restricted) })
    }

    /// Attribute list carrying the job and the exact set of inheritable
    /// handles.
    ///
    /// Two lifetime rules shape this type. The list itself needs pointer
    /// alignment, which a `Vec<u8>` would not guarantee, hence `Vec<usize>`.
    /// And every value handed to `UpdateProcThreadAttribute` is stored by
    /// pointer, so it must stay alive until the list is destroyed: the job and
    /// handle arrays live in this struct, in heap buffers that survive a move.
    struct Attributes {
        storage: Vec<usize>,
        jobs: Vec<HANDLE>,
        handles: Vec<HANDLE>,
        capabilities: Box<SECURITY_CAPABILITIES>,
        app_packages_policy: Box<u32>,
        list: LPPROC_THREAD_ATTRIBUTE_LIST,
    }

    impl Attributes {
        fn new(
            job: HANDLE,
            handles: &[HANDLE],
            sid: windows_sys::Win32::Security::PSID,
        ) -> Result<Self> {
            let mut bytes = 0usize;
            unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 4, 0, &mut bytes) };
            if bytes == 0 {
                return Err(failed("InitializeProcThreadAttributeList (dimensione)"));
            }
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let list = storage.as_mut_ptr().cast::<core::ffi::c_void>();
            if unsafe { InitializeProcThreadAttributeList(list, 4, 0, &mut bytes) } == 0 {
                return Err(failed("InitializeProcThreadAttributeList"));
            }
            // Built only once the list exists, so `Drop` never deletes an
            // uninitialised list.
            let attributes = Self {
                storage,
                jobs: vec![job],
                handles: handles.to_vec(),
                capabilities: Box::new(SECURITY_CAPABILITIES {
                    AppContainerSid: sid,
                    ..Default::default()
                }),
                // PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT (WinNT.h).
                app_packages_policy: Box::new(1),
                list,
            };
            for (kind, pointer, size, what) in [
                (
                    windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
                    (&*attributes.app_packages_policy as *const u32).cast::<core::ffi::c_void>(),
                    size_of::<u32>(),
                    "LPAC",
                ),
                (
                    PROC_THREAD_ATTRIBUTE_JOB_LIST,
                    attributes.jobs.as_ptr().cast::<core::ffi::c_void>(),
                    size_of_val(&attributes.jobs[..]),
                    "job",
                ),
                (
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                    attributes.handles.as_ptr().cast::<core::ffi::c_void>(),
                    size_of_val(&attributes.handles[..]),
                    "handle",
                ),
                (
                    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
                    (&*attributes.capabilities as *const SECURITY_CAPABILITIES).cast(),
                    size_of::<SECURITY_CAPABILITIES>(),
                    "AppContainer senza capabilities",
                ),
            ] {
                if unsafe {
                    UpdateProcThreadAttribute(
                        attributes.list,
                        0,
                        kind as usize,
                        pointer.cast(),
                        size,
                        ptr::null_mut(),
                        ptr::null(),
                    )
                } == 0
                {
                    return Err(failed(&format!("UpdateProcThreadAttribute ({what})")));
                }
            }
            Ok(attributes)
        }
    }

    impl Drop for Attributes {
        fn drop(&mut self) {
            unsafe { DeleteProcThreadAttributeList(self.list) };
            self.storage.clear();
        }
    }

    /// A worker created inside the job, under a restricted token, holding
    /// nothing it did not receive explicitly.
    pub struct Confined {
        process: Option<OwnedHandle>,
        input: Option<File>,
        output: Option<File>,
        _runtime: super::appcontainer::Runtime,
    }
    // The handles are owned exclusively and only passed to kernel calls.
    unsafe impl Send for Confined {}

    impl Job {
        pub fn spawn(&self, binary: &Path, working_directory: &Path) -> Result<Confined> {
            let job = self.0.context("Avvio confinato senza job")?;
            let _ = working_directory;
            let runtime = super::appcontainer::Runtime::new(binary)?;
            let to_child = pipe(true)?;
            let from_child = pipe(false)?;
            // Rust's own stderr goes nowhere: the worker reports through the
            // protocol, and a console handle would be one more thing inherited.
            let null = File::options()
                .write(true)
                .open("NUL")
                .context("Apertura di NUL per stderr")?;
            let null = OwnedHandle::from(null);
            if unsafe { SetHandleInformation(null.as_raw_handle(), HANDLE_FLAG_INHERIT, 1) } == 0 {
                return Err(failed("SetHandleInformation (NUL)"));
            }

            let inheritable = [
                to_child.theirs.as_raw_handle(),
                from_child.theirs.as_raw_handle(),
                null.as_raw_handle(),
            ];
            let attributes = Attributes::new(job, &inheritable, runtime.sid())?;
            let token = restricted_token()?;

            let startup = STARTUPINFOEXW {
                StartupInfo: STARTUPINFOW {
                    cb: size_of::<STARTUPINFOEXW>() as u32,
                    dwFlags: STARTF_USESTDHANDLES,
                    hStdInput: to_child.theirs.as_raw_handle(),
                    hStdOutput: from_child.theirs.as_raw_handle(),
                    hStdError: null.as_raw_handle(),
                    ..Default::default()
                },
                lpAttributeList: attributes.list,
            };
            // Quoted so a path with spaces stays one argument. The flag states
            // that this process was confined; the worker still asks the kernel
            // to confirm it before widening what it will decode.
            let application = wide(runtime.binary().as_os_str());
            let mut command: Vec<u16> = std::iter::once(b'"' as u16)
                .chain(application[..application.len() - 1].iter().copied())
                .chain("\" --confined".encode_utf16())
                .chain(Some(0))
                .collect();
            let directory = wide(runtime.directory().as_os_str());
            let environment = super::appcontainer::environment(token.as_raw_handle())?;
            let mut information = PROCESS_INFORMATION::default();
            let created = unsafe {
                CreateProcessAsUserW(
                    token.as_raw_handle(),
                    application.as_ptr(),
                    command.as_mut_ptr(),
                    ptr::null(),
                    ptr::null(),
                    1,
                    EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
                    environment.as_ptr().cast(),
                    directory.as_ptr(),
                    &startup.StartupInfo,
                    &mut information,
                )
            };
            if created == 0 {
                return Err(failed("CreateProcessAsUser"))
                    .context("Avvio confinato di tr-worker; compilare entrambi i binari");
            }
            // SAFETY: creation succeeded, so both handles are owned. The thread
            // handle has no further use and is closed at once.
            unsafe {
                let process = OwnedHandle::from_raw_handle(information.hProcess);
                CloseHandle(information.hThread);
                Ok(Confined {
                    process: Some(process),
                    input: Some(File::from(to_child.ours)),
                    output: Some(File::from(from_child.ours)),
                    _runtime: runtime,
                })
            }
            // The child ends drop here, so only the worker holds them.
        }
    }

    impl Confined {
        pub fn take_pipes(&mut self) -> Result<(Box<dyn Write + Send>, Box<dyn Read + Send>)> {
            Ok((
                Box::new(self.input.take().context("stdin worker già preso")?),
                Box::new(self.output.take().context("stdout worker già preso")?),
            ))
        }

        pub fn terminate(&mut self) {
            if let Some(process) = self.process.take() {
                // Closing the pipes first lets a healthy worker see EOF and
                // leave on its own; the kill covers one that does not.
                self.input.take();
                self.output.take();
                unsafe {
                    TerminateProcess(process.as_raw_handle(), 1);
                    WaitForSingleObject(process.as_raw_handle(), 5_000);
                }
            }
        }

        pub fn exited(&mut self) -> Option<i32> {
            let process = self.process.as_ref()?;
            let mut code = 0u32;
            if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
                return None;
            }
            (code != STILL_ACTIVE as u32).then_some(code as i32)
        }
    }
}
