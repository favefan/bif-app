// SPDX-License-Identifier: Apache-2.0
//! Windows ownership and credential APIs. No secrets enter arguments or files.
use std::{ffi::c_void, io, mem, os::windows::io::AsRawHandle, process::Child, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_NOT_FOUND, HANDLE, INVALID_HANDLE_VALUE},
    Security::Credentials::*,
    System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
    UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
};
use zeroize::Zeroizing;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
pub fn test_id() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[derive(Clone, Copy)]
struct Listener {
    pid: u32,
    port: u16,
    address: u32,
}

fn listeners() -> io::Result<Vec<Listener>> {
    use windows_sys::Win32::{
        Foundation::ERROR_INSUFFICIENT_BUFFER, NetworkManagement::IpHelper::*,
        Networking::WinSock::AF_INET,
    };
    unsafe {
        let mut size = 0;
        let status = GetExtendedTcpTable(
            ptr::null_mut(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if status != 0 && status != ERROR_INSUFFICIENT_BUFFER {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        for _ in 0..4 {
            let mut buffer = vec![
                0u64;
                (size as usize)
                    .max(mem::size_of::<MIB_TCPTABLE_OWNER_PID>())
                    .div_ceil(8)
            ];
            let status = GetExtendedTcpTable(
                buffer.as_mut_ptr().cast(),
                &mut size,
                0,
                AF_INET as u32,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            );
            if status == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            let table = &*(buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            let needed = mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table)
                + table.dwNumEntries as usize * mem::size_of::<MIB_TCPROW_OWNER_PID>();
            if needed > buffer.len() * 8 {
                return Err(io::Error::other("Invalid Windows listener table size"));
            }
            return Ok(std::slice::from_raw_parts(
                table.table.as_ptr(),
                table.dwNumEntries as usize,
            )
            .iter()
            .map(|r| Listener {
                pid: r.dwOwningPid,
                port: u16::from_be(r.dwLocalPort as u16),
                address: r.dwLocalAddr,
            })
            .collect());
        }
        Err(io::Error::other(
            "Windows listener table changed during inspection",
        ))
    }
}
fn overlaps(row: &Listener, port: u16, address: std::net::Ipv4Addr) -> bool {
    row.port == port
        && (address.is_unspecified()
            || row.address == 0
            || row.address == u32::from_ne_bytes(address.octets()))
}
/// Windows can accept a wildcard bind while a specific-address listener already
/// owns that port. Such a listener can shadow localhost, so bind success alone
/// is not sufficient evidence that a LAN port is available.
pub fn port_in_use(port: u16, address: std::net::Ipv4Addr) -> io::Result<bool> {
    Ok(listeners()?.iter().any(|r| overlaps(r, port, address)))
}
/// Readiness requires our exact binding AND no overlapping foreign listener.
/// In particular, an owned 0.0.0.0 socket must not trust another PID's /health
/// response on 127.0.0.1 at the same port.
pub fn owns_listener(pid: u32, port: u16, address: std::net::Ipv4Addr) -> bool {
    let Ok(rows) = listeners() else {
        return false;
    };
    rows.iter().any(|r| {
        r.pid == pid && r.port == port && r.address == u32::from_ne_bytes(address.octets())
    }) && !rows
        .iter()
        .any(|r| r.pid != pid && overlaps(r, port, address))
}

pub fn error_dialog(message: &str) {
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide(message).as_ptr(),
            wide("bif-app").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub trait CredentialStore {
    fn read(&self) -> Result<Option<Zeroizing<String>>, String>;
    fn write(&self, value: &str) -> Result<(), String>;
}

pub struct WindowsCredentials(pub String);
#[cfg(test)]
impl WindowsCredentials {
    pub fn delete_test_credential(&self) {
        assert!(self.0.starts_with("bif-app/smoke/") || self.0.starts_with("bif-app/test/"));
        unsafe {
            assert_ne!(CredDeleteW(wide(&self.0).as_ptr(), CRED_TYPE_GENERIC, 0), 0);
        }
    }
}
impl CredentialStore for WindowsCredentials {
    fn read(&self) -> Result<Option<Zeroizing<String>>, String> {
        let mut credential: *mut CREDENTIALW = ptr::null_mut();
        unsafe {
            if CredReadW(
                wide(&self.0).as_ptr(),
                CRED_TYPE_GENERIC,
                0,
                &mut credential,
            ) == 0
            {
                return if GetLastError() == ERROR_NOT_FOUND {
                    Ok(None)
                } else {
                    Err("Windows Credential Manager could not read the gateway key. Unlock your Windows profile and retry.".into())
                };
            }
            let bytes = Zeroizing::new(
                std::slice::from_raw_parts(
                    (*credential).CredentialBlob,
                    (*credential).CredentialBlobSize as usize,
                )
                .to_vec(),
            );
            CredFree(credential.cast::<c_void>());
            let value = std::str::from_utf8(&bytes).map_err(|_| {
                "The stored gateway key is invalid; restore the original Windows credential."
            })?;
            Ok(Some(Zeroizing::new(value.to_owned())))
        }
    }
    fn write(&self, value: &str) -> Result<(), String> {
        let mut target = wide(&self.0);
        let mut user = wide("bif-app");
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: value.len() as u32,
            CredentialBlob: value.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: user.as_mut_ptr(),
            ..unsafe { mem::zeroed() }
        };
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err("Windows Credential Manager could not save the gateway key. No plaintext fallback is permitted.".into());
        }
        Ok(())
    }
}

pub fn encryption_key(
    store: &impl CredentialStore,
    existing_data: bool,
) -> Result<Zeroizing<String>, String> {
    if let Some(key) = store.read()? {
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("The stored gateway encryption key is invalid.".into());
        }
        return Ok(key);
    }
    if existing_data {
        return Err("The gateway encryption key is missing from Windows Credential Manager, but Bifrost data already exists. Restore the original credential; bif-app will not replace the key or alter your data.".into());
    }
    let mut random = Zeroizing::new([0u8; 32]);
    getrandom::fill(random.as_mut()).map_err(|_| "Windows secure random generation failed")?;
    let mut key = Zeroizing::new(String::with_capacity(64));
    use std::fmt::Write;
    for byte in random.iter() {
        write!(&mut *key, "{byte:02x}").unwrap();
    }
    store.write(&key)?;
    let saved = store
        .read()?
        .ok_or("Windows did not retain the encryption key")?;
    if *saved != *key {
        return Err("Windows credential verification failed".into());
    }
    Ok(key)
}

pub struct Job(HANDLE);
unsafe impl Send for Job {}
impl Job {
    pub fn new() -> io::Result<Self> {
        unsafe {
            let handle = CreateJobObjectW(ptr::null(), ptr::null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const c_void,
                mem::size_of_val(&limits) as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }
    /// Child is created suspended, attached to the job, then resumed: there is no
    /// interval in which a running gateway can escape its owner's job.
    pub fn attach_and_resume(&self, child: &Child) -> io::Result<()> {
        unsafe {
            if AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) == 0 {
                return Err(io::Error::last_os_error());
            }
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let mut entry: THREADENTRY32 = mem::zeroed();
            entry.dwSize = mem::size_of_val(&entry) as u32;
            let mut found = false;
            let mut next = Thread32First(snapshot, &mut entry);
            while next != 0 {
                if entry.th32OwnerProcessID == child.id() {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        found = ResumeThread(thread) != u32::MAX;
                        CloseHandle(thread);
                    }
                    break;
                }
                next = Thread32Next(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
            if !found {
                return Err(io::Error::other("Cannot resume the owned gateway process"));
            }
            Ok(())
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Memory(RefCell<Option<String>>, bool);
    impl CredentialStore for Memory {
        fn read(&self) -> Result<Option<Zeroizing<String>>, String> {
            if self.1 {
                Err("locked".into())
            } else {
                Ok(self.0.borrow().clone().map(Zeroizing::new))
            }
        }
        fn write(&self, v: &str) -> Result<(), String> {
            *self.0.borrow_mut() = Some(v.into());
            Ok(())
        }
    }
    #[test]
    fn key_is_created_once_and_reused() {
        let store = Memory(RefCell::new(None), false);
        let a = encryption_key(&store, false).unwrap();
        assert_eq!(a.len(), 64);
        assert_eq!(a, encryption_key(&store, true).unwrap());
    }
    #[test]
    fn missing_or_locked_credentials_fail_closed() {
        assert!(encryption_key(&Memory(RefCell::new(None), false), true).is_err());
        assert!(encryption_key(&Memory(RefCell::new(None), true), false).is_err());
        assert!(encryption_key(&Memory(RefCell::new(Some("bad".into())), false), false).is_err());
    }
    #[test]
    fn actual_windows_credential_roundtrip() {
        let store = WindowsCredentials(format!("bif-app/test/{}", std::process::id()));
        let a = encryption_key(&store, false).unwrap();
        assert_eq!(a, encryption_key(&store, true).unwrap());
        unsafe {
            assert_ne!(
                CredDeleteW(wide(&store.0).as_ptr(), CRED_TYPE_GENERIC, 0),
                0
            );
        }
    }
}
