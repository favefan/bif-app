// SPDX-License-Identifier: Apache-2.0
use crate::platform::Job;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpListener},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Paths {
    pub data: PathBuf,
    pub desktop: PathBuf,
}
impl Paths {
    pub fn from_local(local: &Path) -> Self {
        let root = local.join("bif-app");
        Self {
            data: root.join("bifrost"),
            desktop: root.join("desktop"),
        }
    }
    pub fn create(&self) -> io::Result<()> {
        fs::create_dir_all(&self.data)?;
        fs::create_dir_all(&self.desktop)
    }
    pub fn has_data(&self) -> io::Result<bool> {
        Ok(fs::read_dir(&self.data)?.next().is_some())
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct State {
    pub port: Option<u16>,
}
pub fn read_state(dir: &Path) -> State {
    fs::read(dir.join("state.json"))
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or_default()
}
pub fn save_state(dir: &Path, port: u16) -> io::Result<()> {
    let temp = dir.join("state.json.tmp");
    fs::write(
        &temp,
        serde_json::to_vec_pretty(&State { port: Some(port) })?,
    )?;
    fs::rename(temp, dir.join("state.json"))
}
pub fn http_client() -> Result<reqwest::blocking::Client, reqwest::Error> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
}
pub fn healthy(client: &reqwest::blocking::Client, port: u16) -> bool {
    client
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .is_ok_and(|r| r.status() == reqwest::StatusCode::OK)
}
/// A responding Bifrost is still not proof of ownership. Never attach to it.
/// Holding the selected socket until immediately before spawn narrows the bind race.
pub fn reserve_port(
    preferred: Option<u16>,
    client: &reqwest::blocking::Client,
) -> io::Result<TcpListener> {
    let ports = preferred
        .filter(|p| (8080..=8180).contains(p))
        .into_iter()
        .chain(8080..=8180);
    for port in ports {
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
            Ok(listener) => return Ok(listener),
            Err(_) => {
                let _responds_to_health = healthy(client, port);
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "No free localhost port in 8080–8180",
    ))
}

pub fn command(exe: &Path, paths: &Paths, port: u16, key: &str) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(["-host", "127.0.0.1", "-port", &port.to_string(), "-app-dir"])
        .arg(&paths.data)
        .args(["-shutdown-on-stdin-close", "-log-style", "json"])
        .current_dir(&paths.data)
        .env("BIFROST_ENCRYPTION_KEY", key)
        .env_remove("BIFROST_UI_DEV")
        .env_remove("BIFROST_CONFIG_FILE")
        .env_remove("BIFROST_PROFILER")
        .env_remove("BIFROST_PPROF_PORT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    cmd
}

pub struct Gateway {
    pub child: Child,
    _job: Option<Job>,
    readers: Vec<thread::JoinHandle<()>>,
    pub port: u16,
}
impl Gateway {
    pub fn spawn(
        exe: &Path,
        paths: &Paths,
        port: u16,
        key: Zeroizing<String>,
    ) -> Result<Self, String> {
        let job = Job::new().map_err(|e| format!("Cannot create gateway ownership job: {e}"))?;
        let log_path = paths.desktop.join("gateway.log");
        if fs::metadata(&log_path).is_ok_and(|m| m.len() > 5 * 1024 * 1024) {
            let _ = fs::remove_file(paths.desktop.join("gateway.previous.log"));
            fs::rename(&log_path, paths.desktop.join("gateway.previous.log"))
                .map_err(|e| e.to_string())?;
        }
        let log = Arc::new(Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_path)
                .map_err(|e| e.to_string())?,
        ));
        let mut child = command(exe, paths, port, &key)
            .spawn()
            .map_err(|e| format!("Cannot start the bundled Bifrost gateway: {e}"))?;
        if let Err(e) = job.attach_and_resume(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Cannot safely own the gateway process: {e}"));
        }
        let secret = Arc::new(key);
        let readers = vec![
            log_pipe(child.stdout.take().unwrap(), log.clone(), secret.clone()),
            log_pipe(child.stderr.take().unwrap(), log, secret),
        ];
        Ok(Self {
            child,
            _job: Some(job),
            readers,
            port,
        })
    }
    pub fn wait_ready(
        &mut self,
        client: &reqwest::blocking::Client,
        timeout: Duration,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                return Err(format!(
                    "Bifrost exited during startup ({status}). See desktop/gateway.log."
                ));
            }
            if crate::platform::owns_listener(self.child.id(), self.port)
                && healthy(client, self.port)
            {
                // Require our child to remain alive after the probe. Unknown listeners
                // are never intentionally reused, including during bind races.
                thread::sleep(Duration::from_millis(150));
                if self.child.try_wait().map_err(|e| e.to_string())?.is_none() {
                    return Ok(());
                }
            }
            thread::sleep(Duration::from_millis(200));
        }
        Err("Bifrost did not become healthy within 90 seconds. See desktop/gateway.log.".into())
    }
    pub fn stop(&mut self, timeout: Duration) -> Result<bool, String> {
        drop(self.child.stdin.take());
        let deadline = Instant::now() + timeout;
        loop {
            if self.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                for reader in self.readers.drain(..) {
                    let _ = reader.join();
                }
                return Ok(true);
            }
            if Instant::now() >= deadline {
                self.child.kill().map_err(|e| e.to_string())?;
                let _ = self.child.wait();
                return Ok(false);
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}
impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.stop(Duration::from_secs(2));
    }
}
fn log_pipe(
    reader: impl io::Read + Send + 'static,
    file: Arc<Mutex<File>>,
    key: Arc<Zeroizing<String>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            if let Ok(mut log) = file.lock() {
                let _ = writeln!(log, "{}", line.replace(key.as_str(), "[REDACTED]"));
            }
        }
    })
}

#[cfg(test)]
mod smoke {
    use super::*;
    use crate::platform::{encryption_key, WindowsCredentials};

    #[test]
    #[ignore = "requires the freshly built BIF_APP_TEST_SIDECAR"]
    fn real_gateway_ui_api_graceful_restart_and_job_cleanup() {
        let exe =
            PathBuf::from(std::env::var("BIF_APP_TEST_SIDECAR").expect("set BIF_APP_TEST_SIDECAR"));
        let local = std::env::temp_dir().join(format!("bif-app-smoke-{}", std::process::id()));
        let paths = Paths::from_local(&local);
        paths.create().unwrap();
        let store = WindowsCredentials(format!("bif-app/smoke/{}", std::process::id()));
        let key = encryption_key(&store, paths.has_data().unwrap()).unwrap();
        let client = http_client().unwrap();
        let socket = reserve_port(None, &client).unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        let mut gateway = Gateway::spawn(&exe, &paths, port, key.clone()).unwrap();
        gateway
            .wait_ready(&client, Duration::from_secs(90))
            .unwrap();
        assert!(crate::platform::owns_listener(gateway.child.id(), port));
        assert!(!crate::platform::owns_listener(std::process::id(), port));
        let base = format!("http://127.0.0.1:{port}");
        let root = client.get(&base).send().unwrap();
        assert_eq!(root.status(), 200);
        let html = root.text().unwrap();
        assert!(
            html.contains("<script") && html.contains("/assets/"),
            "must serve real embedded UI"
        );
        for path in ["/api/providers", "/api/config", "/v1/models"] {
            let r = client.get(format!("{base}{path}")).send().unwrap();
            assert_eq!(r.status(), 200, "{path}: {}", r.text().unwrap());
        }
        // Persist a local-only provider using the existing management API. It
        // points at an unused loopback port; no real provider is contacted.
        let r = client.post(format!("{base}/api/providers")).json(&serde_json::json!({
            "provider": "ollama", "network_config": { "base_url": "http://127.0.0.1:1", "default_request_timeout_in_seconds": 1 }
        })).send().unwrap();
        assert!(
            r.status().is_success(),
            "create provider: {}",
            r.text().unwrap()
        );
        save_state(&paths.desktop, port).unwrap();
        assert!(
            gateway.stop(Duration::from_secs(40)).unwrap(),
            "graceful exit required, not forced kill"
        );
        assert!(!healthy(&client, port));
        assert!(paths.has_data().unwrap());
        assert_eq!(key, encryption_key(&store, true).unwrap());
        let log = fs::read_to_string(paths.desktop.join("gateway.log")).unwrap();
        assert!(
            log.contains("cleanup completed"),
            "Bifrost must complete its own storage cleanup"
        );
        assert!(!log.contains(key.as_str()));
        fn no_key(dir: &Path, key: &str) {
            for entry in fs::read_dir(dir).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    no_key(&p, key);
                } else {
                    let b = fs::read(&p).unwrap();
                    assert!(
                        !b.windows(key.len()).any(|v| v == key.as_bytes()),
                        "plaintext encryption key in {}",
                        p.display()
                    );
                }
            }
        }
        no_key(&paths.data, &key);
        let mut again = Gateway::spawn(
            &exe,
            &paths,
            read_state(&paths.desktop).port.unwrap(),
            encryption_key(&store, true).unwrap(),
        )
        .unwrap();
        again.wait_ready(&client, Duration::from_secs(90)).unwrap();
        let providers: serde_json::Value = client
            .get(format!("{base}/api/providers"))
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert!(
            providers.to_string().contains("ollama"),
            "provider did not survive restart"
        );
        // Closing the job emulates a desktop crash: the child cannot outlive it.
        drop(again._job.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        while again.child.try_wait().unwrap().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(100));
        }
        assert!(again.child.try_wait().unwrap().is_some());
        assert!(!healthy(&client, port));
        // Test-owned credential and data only. Never touches the user's profile.
        store.delete_test_credential();
        fs::remove_dir_all(&local).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_data_is_separate() {
        let paths = Paths::from_local(Path::new("C:/Users/test/AppData/Local"));
        assert!(paths.data.ends_with("bif-app/bifrost"));
        assert!(paths.desktop.ends_with("bif-app/desktop"));
    }
    #[test]
    fn command_uses_loopback_and_environment_secret() {
        let paths = Paths::from_local(Path::new("C:/test space"));
        let cmd = command(Path::new("gateway.exe"), &paths, 8091, "test-secret");
        let args: Vec<_> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"127.0.0.1".into()));
        assert!(args.contains(&"8091".into()));
        assert!(!args.iter().any(|s| s.contains("test-secret")));
        assert!(cmd
            .get_envs()
            .any(|(k, v)| k == "BIFROST_ENCRYPTION_KEY" && v.unwrap() == "test-secret"));
    }
    #[test]
    fn persisted_port_and_conflict_selection() {
        let client = http_client().unwrap();
        let held = reserve_port(Some(8178), &client).unwrap();
        let p = held.local_addr().unwrap().port();
        let next = reserve_port(Some(p), &client).unwrap();
        assert_ne!(next.local_addr().unwrap().port(), p);
        drop(held);
        let selected = reserve_port(Some(p), &client).unwrap();
        assert_eq!(selected.local_addr().unwrap().port(), p);
        let dir = std::env::temp_dir().join(format!("bif-app-state-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        save_state(&dir, p).unwrap();
        assert_eq!(read_state(&dir).port, Some(p));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn health_checks_require_http_200() {
        for (status, expected) in [(200, true), (503, false), (302, false)] {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let p = listener.local_addr().unwrap().port();
            let server = thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(s.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                assert!(line.starts_with("GET /health "));
                write!(
                    s,
                    "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            });
            assert_eq!(healthy(&http_client().unwrap(), p), expected);
            server.join().unwrap();
        }
    }
}
