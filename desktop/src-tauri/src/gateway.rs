// SPDX-License-Identifier: Apache-2.0
use crate::platform::Job;
use crate::settings::Settings;
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
    settings: &Settings,
    preferred: Option<u16>,
    client: &reqwest::blocking::Client,
) -> io::Result<TcpListener> {
    settings.validate().map_err(io::Error::other)?;
    let ports = settings.ports(preferred);
    for port in ports {
        match TcpListener::bind((settings.address(), port)) {
            Ok(listener) => return Ok(listener),
            Err(_) => {
                let _responds_to_health = healthy(client, port);
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        format!(
            "监听地址 {} 上端口 {} 不可用{}。",
            settings.host,
            settings.preferred_port,
            if settings.auto_port {
                "，附近 100 个端口也不可用"
            } else {
                "（自动换用端口已关闭）"
            }
        ),
    ))
}

pub fn command(exe: &Path, paths: &Paths, host: &str, port: u16, key: &str) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(["-host", host, "-port", &port.to_string(), "-app-dir"])
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
    host: Ipv4Addr,
}
impl Gateway {
    pub fn spawn(
        exe: &Path,
        paths: &Paths,
        settings: &Settings,
        port: u16,
        key: Zeroizing<String>,
    ) -> Result<Self, String> {
        settings.validate()?;
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
        let mut child = command(exe, paths, &settings.host, port, &key)
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
            host: settings.address(),
        })
    }
    #[cfg(test)]
    pub fn wait_ready(
        &mut self,
        client: &reqwest::blocking::Client,
        timeout: Duration,
    ) -> Result<(), String> {
        self.wait_ready_until(client, timeout, || false)
    }
    pub fn wait_ready_until(
        &mut self,
        client: &reqwest::blocking::Client,
        timeout: Duration,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if cancelled() {
                return Err("Startup cancelled".into());
            }
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                return Err(format!(
                    "Bifrost exited during startup ({status}). See desktop/gateway.log."
                ));
            }
            if crate::platform::owns_listener(self.child.id(), self.port, self.host)
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
                // Reap any remaining MCP descendants before joining pipe readers;
                // descendants can otherwise keep inherited stdout handles open.
                drop(self._job.take());
                for reader in self.readers.drain(..) {
                    let _ = reader.join();
                }
                return Ok(true);
            }
            if Instant::now() >= deadline {
                self.child.kill().map_err(|e| e.to_string())?;
                let _ = self.child.wait();
                drop(self._job.take());
                for reader in self.readers.drain(..) {
                    let _ = reader.join();
                }
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
    use std::{
        io::Read,
        sync::atomic::{AtomicBool, Ordering},
    };

    struct LocalProvider {
        port: u16,
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
    }
    impl LocalProvider {
        fn start() -> Self {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            listener.set_nonblocking(true).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let stopping = stop.clone();
            let worker = thread::spawn(move || {
                while !stopping.load(Ordering::SeqCst) {
                    let Ok((mut socket, _)) = listener.accept() else {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    };
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut reader = BufReader::new(socket.try_clone().unwrap());
                    let mut first = String::new();
                    if reader.read_line(&mut first).is_err() {
                        continue;
                    }
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            length = v.trim().parse::<usize>().unwrap();
                        }
                    }
                    let mut body = vec![0; length];
                    let _ = reader.read_exact(&mut body);
                    let response = if first.contains("chat/completions") {
                        r#"{"id":"chatcmpl-local","object":"chat.completion","created":1,"model":"desktop-smoke","choices":[{"index":0,"message":{"role":"assistant","content":"local smoke response"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":3,"total_tokens":4}}"#
                    } else {
                        r#"{"object":"list","data":[{"id":"desktop-smoke","object":"model","created":1,"owned_by":"local"}]}"#
                    };
                    let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len());
                }
            });
            Self {
                port,
                stop,
                worker: Some(worker),
            }
        }
    }
    impl Drop for LocalProvider {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(w) = self.worker.take() {
                let _ = w.join();
            }
        }
    }

    #[test]
    #[ignore = "requires the freshly built BIF_APP_TEST_SIDECAR"]
    fn real_gateway_ui_api_graceful_restart_and_job_cleanup() {
        let exe =
            PathBuf::from(std::env::var("BIF_APP_TEST_SIDECAR").expect("set BIF_APP_TEST_SIDECAR"));
        let local =
            std::env::temp_dir().join(format!("bif-app-smoke-{}", crate::platform::test_id()));
        let paths = Paths::from_local(&local);
        paths.create().unwrap();
        let store = WindowsCredentials(format!("bif-app/smoke/{}", crate::platform::test_id()));
        let key = encryption_key(&store, paths.has_data().unwrap()).unwrap();
        let client = http_client().unwrap();
        let socket = reserve_port(&Settings::default(), None, &client).unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        let mut gateway =
            Gateway::spawn(&exe, &paths, &Settings::default(), port, key.clone()).unwrap();
        gateway
            .wait_ready(&client, Duration::from_secs(90))
            .unwrap();
        assert!(crate::platform::owns_listener(
            gateway.child.id(),
            port,
            Ipv4Addr::LOCALHOST
        ));
        assert!(!crate::platform::owns_listener(
            std::process::id(),
            port,
            Ipv4Addr::LOCALHOST
        ));
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
        // Exercise the unmodified OpenAI-compatible API against a loopback
        // fixture, without credentials or requests to a paid service.
        let fixture = LocalProvider::start();
        let r = client.post(format!("{base}/api/providers")).json(&serde_json::json!({
            "provider": "ollama", "network_config": { "base_url": format!("http://127.0.0.1:{}", fixture.port), "default_request_timeout_in_seconds": 2 }
        })).send().unwrap();
        assert!(
            r.status().is_success(),
            "create provider: {}",
            r.text().unwrap()
        );
        let r = client.post(format!("{base}/api/providers/ollama/keys")).json(&serde_json::json!({
            "name": "desktop-smoke", "value": "desktop-smoke-not-a-real-provider-key", "models": ["desktop-smoke"], "weight": 1,
            "ollama_key_config": { "url": format!("http://127.0.0.1:{}", fixture.port) }
        })).send().unwrap();
        assert!(
            r.status().is_success(),
            "create fixture key: {}",
            r.text().unwrap()
        );
        let r = client.post(format!("{base}/v1/chat/completions")).json(&serde_json::json!({
            "model": "ollama/desktop-smoke", "messages": [{"role":"user","content":"local smoke"}]
        })).send().unwrap();
        assert_eq!(
            r.status(),
            200,
            "OpenAI-compatible chat: {}",
            r.text().unwrap()
        );
        let response: serde_json::Value = r.json().unwrap();
        assert_eq!(
            response["choices"][0]["message"]["content"],
            "local smoke response"
        );
        crate::settings::save_state(
            &paths.desktop,
            &crate::settings::State {
                settings: Settings::default(),
                port: Some(port),
            },
        )
        .unwrap();
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
            &Settings::default(),
            crate::settings::read_state(&paths.desktop)
                .unwrap()
                .port
                .unwrap(),
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
        again.stop(Duration::from_secs(2)).unwrap();
        store.delete_test_credential();
        fs::remove_dir_all(&local).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn early_exit_and_timeout_are_reported_and_reaped() {
        let dir =
            std::env::temp_dir().join(format!("bif-app-exit-test-{}", crate::platform::test_id()));
        let paths = Paths::from_local(&dir);
        paths.create().unwrap();
        // The Rust test executable rejects Bifrost's CLI flags immediately.
        // This is a real child in the same Windows Job lifecycle as production.
        let mut g = Gateway::spawn(
            &std::env::current_exe().unwrap(),
            &paths,
            &Settings::default(),
            8180,
            Zeroizing::new("test-secret".into()),
        )
        .unwrap();
        let error = g
            .wait_ready(&http_client().unwrap(), Duration::from_secs(10))
            .unwrap_err();
        assert!(error.contains("exited during startup"), "{error}");
        assert!(g.stop(Duration::from_secs(1)).unwrap());
        let timeout = g
            .wait_ready(&http_client().unwrap(), Duration::ZERO)
            .unwrap_err();
        assert!(timeout.contains("did not become healthy"));
        drop(g);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn local_data_is_separate() {
        let paths = Paths::from_local(Path::new("C:/Users/test/AppData/Local"));
        assert!(paths.data.ends_with("bif-app/bifrost"));
        assert!(paths.desktop.ends_with("bif-app/desktop"));
    }
    #[test]
    fn command_uses_loopback_and_environment_secret() {
        let paths = Paths::from_local(Path::new("C:/test space"));
        let cmd = command(
            Path::new("gateway.exe"),
            &paths,
            "127.0.0.1",
            8091,
            "test-secret",
        );
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
        let held = reserve_port(&Settings::default(), Some(8178), &client).unwrap();
        let p = held.local_addr().unwrap().port();
        let next = reserve_port(&Settings::default(), Some(p), &client).unwrap();
        assert_ne!(next.local_addr().unwrap().port(), p);
        drop(held);
        let selected = reserve_port(&Settings::default(), Some(p), &client).unwrap();
        assert_eq!(selected.local_addr().unwrap().port(), p);
        let dir =
            std::env::temp_dir().join(format!("bif-app-state-{}", crate::platform::test_id()));
        fs::create_dir_all(&dir).unwrap();
        crate::settings::save_state(
            &dir,
            &crate::settings::State {
                settings: Settings::default(),
                port: Some(p),
            },
        )
        .unwrap();
        assert_eq!(crate::settings::read_state(&dir).unwrap().port, Some(p));
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
