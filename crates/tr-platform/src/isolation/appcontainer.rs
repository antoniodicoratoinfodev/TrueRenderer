//! Per-process AppContainer profile and read/execute-only code.
//! No grants on the installation folder, user photographs or application data.
use anyhow::{Context, Result, ensure};
use std::{
    ffi::OsStr,
    fs::File,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        ACL,
        Authorization::{
            EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, SE_FILE_OBJECT,
            SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
        },
        CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, FreeSid,
        Isolation::{CreateAppContainerProfile, DeleteAppContainerProfile},
        OBJECT_INHERIT_ACE, PSID,
    },
    Storage::FileSystem::{FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_SHARE_READ},
};

/// System/profile paths needed by Windows to initialize a lowbox. The user
/// environment is not inherited; secrets, PATH and shell options are excluded.
pub(super) fn environment(token: windows_sys::Win32::Foundation::HANDLE) -> Result<Vec<u16>> {
    use windows_sys::Win32::System::Environment::{
        CreateEnvironmentBlock, DestroyEnvironmentBlock,
    };
    let mut block = ptr::null_mut();
    ensure!(
        unsafe { CreateEnvironmentBlock(&mut block, token, 0) } != 0,
        "Ambiente AppContainer: {}",
        std::io::Error::last_os_error()
    );
    let mut entries = Vec::new();
    // Windows owns a double-NUL-terminated UTF-16 allocation until destroyed.
    unsafe {
        let mut start = block.cast::<u16>();
        while *start != 0 {
            let mut length = 0;
            while *start.add(length) != 0 {
                length += 1;
            }
            let entry = std::slice::from_raw_parts(start, length);
            if let Some(separator) = entry.iter().position(|v| *v == b'=' as u16) {
                let name = String::from_utf16_lossy(&entry[..separator]).to_ascii_uppercase();
                if [
                    "SYSTEMROOT",
                    "SYSTEMDRIVE",
                    "USERPROFILE",
                    "LOCALAPPDATA",
                    "APPDATA",
                    "TEMP",
                    "TMP",
                ]
                .contains(&name.as_str())
                {
                    entries.push((name, entry.to_vec()));
                }
            }
            start = start.add(length + 1);
        }
        DestroyEnvironmentBlock(block);
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut result = Vec::new();
    for (_, entry) in entries {
        result.extend(entry);
        result.push(0);
    }
    result.push(0);
    Ok(result)
}

struct Sid(PSID, Vec<u16>);
impl Drop for Sid {
    fn drop(&mut self) {
        unsafe {
            // Only a profile created successfully by this instance is owned here.
            DeleteAppContainerProfile(self.1.as_ptr());
            FreeSid(self.0);
        }
    }
}
// Exclusively owned allocation; only read until dropped.
unsafe impl Send for Sid {}

pub(super) struct Runtime {
    // Drop the code lease before TempDir removes its own files.
    _code_lease: File,
    directory: tempfile::TempDir,
    sid: Sid,
}
impl Runtime {
    pub(super) fn new(binary: &Path) -> Result<Self> {
        let directory = tempfile::Builder::new().prefix("tr-decoder-").tempdir()?;
        let name = format!(
            "TrueRenderer.Decoder.{}",
            directory.path().file_name().unwrap().to_string_lossy()
        );
        let name = wide(OsStr::new(&name));
        let mut sid = ptr::null_mut();
        let status = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                ptr::null(),
                0,
                &mut sid,
            )
        };
        ensure!(
            status >= 0 && !sid.is_null(),
            "Creazione profilo AppContainer fallita: {status:#x}"
        );
        let sid = Sid(sid, name);
        let path = directory.path().join("tr-worker.exe");
        // Deny modification/deletion while capturing the trusted executable.
        let mut source = File::options()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(binary)
            .context("Apertura eseguibile decoder")?;
        ensure!(
            source.metadata()?.is_file(),
            "Eseguibile decoder non regolare"
        );
        let mut copy = File::create(&path)?;
        std::io::copy(&mut source, &mut copy)?;
        copy.sync_all()?;
        drop(copy);
        grant_code_read(directory.path(), sid.0, true)?;
        grant_code_read(&path, sid.0, false)?;
        let code_lease = File::options()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)?;
        Ok(Self {
            _code_lease: code_lease,
            directory,
            sid,
        })
    }
    pub(super) fn sid(&self) -> PSID {
        self.sid.0
    }
    pub(super) fn binary(&self) -> PathBuf {
        self.directory.path().join("tr-worker.exe")
    }
    pub(super) fn directory(&self) -> &Path {
        self.directory.path()
    }
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

fn grant_code_read(path: &Path, sid: PSID, directory: bool) -> Result<()> {
    let name = wide(path.as_os_str());
    let mut old: *mut ACL = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut old,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    ensure!(
        status == 0,
        "Lettura ACL runtime: {}",
        std::io::Error::from_raw_os_error(status as i32)
    );
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: if directory {
            CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE
        } else {
            0
        },
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: sid.cast(),
            ..Default::default()
        },
    };
    let mut acl = ptr::null_mut();
    let built = unsafe { SetEntriesInAclW(1, &access, old, &mut acl) };
    let applied = if built == 0 {
        unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            )
        }
    } else {
        built
    };
    unsafe {
        if !acl.is_null() {
            LocalFree(acl.cast());
        }
        if !descriptor.is_null() {
            LocalFree(descriptor);
        }
    }
    ensure!(
        applied == 0,
        "ACL runtime AppContainer: {}",
        std::io::Error::from_raw_os_error(applied as i32)
    );
    Ok(())
}
