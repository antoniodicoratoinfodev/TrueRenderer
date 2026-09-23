//! Run explicitly after building: cargo run -p tr-platform --example windows-isolation-probe.
#[cfg(windows)]
#[path = "../../../native/windows/lpac.rs"]
mod lpac;
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        time::Duration,
    };
    use tr_platform::isolation::Isolation;
    fn check(input: &serde_json::Value) -> serde_json::Value {
        let path = input["path"].as_str().unwrap();
        let error = |r: std::io::Result<()>| r.err().and_then(|e| e.raw_os_error()).unwrap_or(0);
        let read = error(std::fs::read(path).map(|_| ()));
        let write = error(
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map(|_| ()),
        );
        let address = input["address"].as_str().unwrap().parse().unwrap();
        use windows_sys::Win32::Networking::WinSock::{WSACleanup, WSADATA, WSAStartup};
        let mut data = WSADATA::default();
        let initialization = unsafe { WSAStartup(0x202, &mut data) };
        // Rust's std panics when Winsock cannot initialize. Observe the OS
        // result first, distinguishing initialization from socket denial.
        let network = if initialization == 0 {
            let result =
                error(TcpStream::connect_timeout(&address, Duration::from_secs(2)).map(|_| ()));
            unsafe { WSACleanup() };
            result
        } else {
            initialization
        };
        serde_json::json!({"read_error":read, "write_error":write, "network_error":network, "winsock_init_error":initialization})
    }
    if std::env::args_os().any(|a| a == "--confined") {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes)?;
        let input: serde_json::Value = serde_json::from_slice(&bytes)?;
        let mut report = check(&input);
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::{
            Security::*,
            System::{JobObjects::*, Threading::*},
        };
        let mut raw_token = std::ptr::null_mut();
        ensure!(
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } != 0,
            "token"
        );
        let token = unsafe { OwnedHandle::from_raw_handle(raw_token) };
        report["lpac_verified"] = lpac::verified(token.as_raw_handle()).into();
        let mut sid = [0u32; 17];
        let mut size = 68;
        ensure!(
            unsafe {
                CreateWellKnownSid(
                    WinBuiltinAnyPackageSid,
                    std::ptr::null_mut(),
                    sid.as_mut_ptr().cast(),
                    &mut size,
                )
            } != 0,
            "sid"
        );
        let mut member = 0;
        let ok = unsafe {
            CheckTokenMembershipEx(
                std::ptr::null_mut(),
                sid.as_mut_ptr().cast(),
                1,
                &mut member,
            )
        };
        report["all_application_packages"] = serde_json::json!({"ok":ok,"member":member});
        for (name, class) in [
            ("appcontainer", TokenIsAppContainer),
            ("lpac", TokenIsLessPrivilegedAppContainer),
        ] {
            let mut value = 0u32;
            let mut returned = 0;
            let ok = unsafe {
                GetTokenInformation(
                    token.as_raw_handle(),
                    class,
                    (&raw mut value).cast(),
                    4,
                    &mut returned,
                )
            };
            report[name] = serde_json::json!({"ok":ok,"value":value,"error":if ok==0 {std::io::Error::last_os_error().raw_os_error()} else {None}});
        }
        let mut lpac = 0u32;
        let mut returned = 0;
        let status = unsafe {
            windows_sys::Wdk::Storage::FileSystem::NtQueryInformationToken(
                token.as_raw_handle(),
                TokenIsLessPrivilegedAppContainer,
                (&raw mut lpac).cast(),
                4,
                &mut returned,
            )
        };
        report["lpac_native"] = serde_json::json!({"status":status,"value":lpac});
        let mut groups = TOKEN_GROUPS::default();
        let mut returned = 0;
        let ok = unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenCapabilities,
                (&raw mut groups).cast(),
                size_of::<TOKEN_GROUPS>() as u32,
                &mut returned,
            )
        };
        report["capabilities"] =
            serde_json::json!({"ok":ok,"count":groups.GroupCount,"bytes":returned});
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        let ok = unsafe {
            QueryInformationJobObject(
                std::ptr::null_mut(),
                JobObjectExtendedLimitInformation,
                (&raw mut limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        report["job"] = serde_json::json!({"ok":ok,"flags":limits.BasicLimitInformation.LimitFlags,"active":limits.BasicLimitInformation.ActiveProcessLimit,"job_memory":limits.JobMemoryLimit,"process_memory":limits.ProcessMemoryLimit});
        let code = std::env::current_exe()?;
        report["code_read"] = std::fs::read(&code).is_ok().into();
        report["runtime_write_error"] =
            std::fs::File::create(code.parent().unwrap().join("forbidden"))
                .err()
                .and_then(|e| e.raw_os_error())
                .unwrap_or(0)
                .into();
        let mut child = std::process::Command::new(&code)
            .arg("--nested-probe")
            .spawn();
        report["child_spawn_denied"] = child.is_err().into();
        if let Ok(child) = child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        serde_json::to_writer(std::io::stdout(), &report)?;
        return Ok(());
    }
    if std::env::args_os().any(|a| a == "--nested-probe") {
        return Ok(());
    }
    if let Some(binary) = std::env::args_os().nth(2)
        && std::env::args_os().nth(1).is_some_and(|a| a == "--worker")
    {
        let isolation = Isolation::bounded(384 * 1024 * 1024)?;
        let mut worker = isolation.spawn(std::path::Path::new(&binary), &std::env::temp_dir())?;
        let (mut send, mut receive) = worker.take_pipes()?;
        let bytes = include_bytes!("../../../corpus/05_Trasparenza.png");
        let request = tr_core::protocol::DecodeRequest {
            raw_engine: Default::default(),
            source_len: bytes.len(),
            max_edge: 32,
            intent: tr_core::protocol::DecodeIntent::Probe,
            edit: None,
            maximum_output_bytes: 1024 * 1024,
        };
        let sent =
            tr_core::protocol::write_control(&mut send, tr_core::protocol::REQUEST, 1, &request)
                .and_then(|_| send.write_all(bytes).map_err(Into::into));
        eprintln!("Worker send: {sent:?}");
        drop(send);
        let read = tr_core::protocol::read_control(&mut receive);
        eprintln!("Worker response: {read:?}");
        std::thread::sleep(Duration::from_millis(100));
        eprintln!("Worker exit: {:?}", worker.exited());
        worker.terminate();
        ensure!(read.is_ok(), "Worker failed");
        return Ok(());
    }
    let data = tempfile::Builder::new()
        .prefix("tr-isolation-proof-")
        .tempdir()?;
    let path = data.path().join("private-sentinel.txt");
    std::fs::write(&path, b"unchanged")?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let input = serde_json::json!({"path":path, "address":listener.local_addr()?.to_string()});
    let control = check(&input);
    ensure!(
        control
            == serde_json::json!({"read_error":0,"write_error":0,"network_error":0,"winsock_init_error":0}),
        "Control failed: {control}"
    );
    let isolation = Isolation::bounded(384 * 1024 * 1024)?;
    let mut worker = isolation.spawn(&std::env::current_exe()?, data.path())?;
    ensure!(isolation.confines(&worker), "Worker outside Job");
    let (mut send, mut receive) = worker.take_pipes()?;
    send.write_all(&serde_json::to_vec(&input)?)?;
    drop(send);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = receive.read_to_end(&mut bytes).map(|_| bytes);
        let _ = tx.send(result);
    });
    let received = rx.recv_timeout(Duration::from_secs(20));
    eprintln!("Worker exit before cleanup: {:?}", worker.exited());
    worker.terminate();
    let report: serde_json::Value = serde_json::from_slice(&received??)?;
    println!(
        "{}",
        serde_json::json!({"control":control,"confined":report})
    );
    ensure!(
        report["read_error"] == 5 && report["write_error"] == 5,
        "Private file access not denied: {report}"
    );
    ensure!(
        report["lpac_verified"] == true,
        "LPAC attribute not verified"
    );
    ensure!(
        report["network_error"] == 10013
            || matches!(report["winsock_init_error"].as_i64(), Some(10106 | 10107)),
        "Network unexpectedly available or unrecognized failure: {report}"
    );
    ensure!(
        report["code_read"] == true && report["runtime_write_error"] == 5,
        "Invalid code ACL: {report}"
    );
    ensure!(
        report["child_spawn_denied"] == true,
        "Child process limit not enforced"
    );
    ensure!(std::fs::read(path)? == b"unchanged", "Sentinel changed");
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows-only AppContainer regression");
}
