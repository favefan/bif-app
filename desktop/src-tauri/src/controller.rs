// SPDX-License-Identifier: Apache-2.0
//! Serialized gateway ownership and transactional desktop-only reconfiguration.
use crate::{
    gateway::{self, Gateway, Paths},
    settings::{self, Settings, State},
};
use std::{path::PathBuf, time::Duration};
use zeroize::Zeroizing;

pub struct Controller {
    pub gateway: Option<Gateway>,
    pub saved: State,
    pub paths: Paths,
    pub exe: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{encryption_key, WindowsCredentials};
    use std::{
        fs,
        net::{Ipv4Addr, TcpListener},
    };

    #[test]
    #[ignore = "requires the freshly built BIF_APP_TEST_SIDECAR"]
    fn real_settings_restart_rollback_persistence_and_lan() {
        let local = std::env::temp_dir().join(format!(
            "bif-app-settings-smoke-{}",
            crate::platform::test_id()
        ));
        let paths = Paths::from_local(&local);
        paths.create().unwrap();
        let store = WindowsCredentials(format!(
            "bif-app/smoke/settings-{}",
            crate::platform::test_id()
        ));
        let key = encryption_key(&store, false).unwrap();
        let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let first = free.local_addr().unwrap().port();
        drop(free);
        let mut c = Controller::new(
            paths.clone(),
            std::env::var("BIF_APP_TEST_SIDECAR").unwrap().into(),
        )
        .unwrap();
        c.saved.settings.preferred_port = first;
        c.saved.settings.auto_port = false;
        c.start(&key, || false).unwrap();
        let client = gateway::http_client().unwrap();
        let original = c.saved.settings.clone();
        let pid = c.gateway.as_ref().unwrap().child.id();
        assert!(c
            .apply(
                Settings {
                    preferred_port: 0,
                    ..original.clone()
                },
                &key,
                || false
            )
            .is_err());
        assert_eq!(
            c.gateway.as_ref().unwrap().child.id(),
            pid,
            "invalid config must not stop the old process"
        );

        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let blocked = occupied.local_addr().unwrap().port();
        let error = c
            .apply(
                Settings {
                    preferred_port: blocked,
                    ..original.clone()
                },
                &key,
                || false,
            )
            .unwrap_err();
        assert!(error.contains("已恢复之前"), "{error}");
        assert!(gateway::healthy(&client, first));
        assert_eq!(
            settings::read_state(&paths.desktop).unwrap().settings,
            original
        );
        assert!(
            occupied.local_addr().is_ok(),
            "unknown listener must not be killed"
        );

        let fallback = Settings {
            preferred_port: blocked,
            auto_port: true,
            ..original.clone()
        };
        c.apply(fallback.clone(), &key, || false).unwrap();
        let actual = c.port().unwrap();
        assert_ne!(actual, blocked);
        assert!(!gateway::healthy(&client, first));
        let disk = settings::read_state(&paths.desktop).unwrap();
        assert_eq!(disk.settings.preferred_port, blocked);
        assert_eq!(disk.port, Some(actual));
        c.stop().unwrap();
        let exe = c.exe.clone();
        c = Controller::new(paths.clone(), exe).unwrap();
        c.start(&key, || false).unwrap();
        assert_eq!(
            c.port(),
            Some(actual),
            "ordinary restart must reuse selected port"
        );

        // Force a state-write failure after the replacement becomes healthy.
        // The old healthy listener must be restored, not left stopped.
        fs::create_dir(paths.desktop.join("state.json.tmp")).unwrap();
        let error = c.apply(original.clone(), &key, || false).unwrap_err();
        assert!(
            error.contains("保存设置失败") && error.contains("已恢复之前"),
            "{error}"
        );
        assert_eq!(c.port(), Some(actual));
        assert!(gateway::healthy(&client, actual));
        fs::remove_dir(paths.desktop.join("state.json.tmp")).unwrap();

        let lan = Settings {
            host: "0.0.0.0".into(),
            preferred_port: actual,
            auto_port: false,
        };
        c.apply(lan, &key, || false).unwrap();
        assert!(gateway::healthy(&client, actual));
        let pid = c.gateway.as_ref().unwrap().child.id();
        assert!(crate::platform::owns_listener(
            pid,
            actual,
            Ipv4Addr::UNSPECIFIED
        ));
        assert!(!crate::platform::owns_listener(
            pid,
            actual,
            Ipv4Addr::LOCALHOST
        ));
        c.apply(original, &key, || false).unwrap();
        assert!(crate::platform::owns_listener(
            c.gateway.as_ref().unwrap().child.id(),
            first,
            Ipv4Addr::LOCALHOST
        ));
        assert_eq!(key, encryption_key(&store, true).unwrap());
        let pid = c.gateway.as_ref().unwrap().child.id();
        assert!(c.apply(fallback, &key, || true).is_err());
        assert_eq!(
            c.gateway.as_ref().unwrap().child.id(),
            pid,
            "cancel before restart must preserve current service"
        );
        c.stop().unwrap();
        assert!(!gateway::healthy(&client, first));
        assert!(!fs::read_to_string(paths.desktop.join("state.json"))
            .unwrap()
            .contains(key.as_str()));
        assert!(!fs::read_to_string(paths.desktop.join("gateway.log"))
            .unwrap()
            .contains(key.as_str()));
        store.delete_test_credential();
        fs::remove_dir_all(local).unwrap();
    }
}
impl Controller {
    pub fn new(paths: Paths, exe: PathBuf) -> Result<Self, String> {
        let saved =
            settings::read_state(&paths.desktop).map_err(|e| format!("无法读取桌面设置：{e}"))?;
        Ok(Self {
            gateway: None,
            saved,
            paths,
            exe,
        })
    }
    pub fn port(&self) -> Option<u16> {
        self.gateway.as_ref().map(|g| g.port)
    }
    fn launch(
        &self,
        config: &Settings,
        last: Option<u16>,
        key: &Zeroizing<String>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Gateway, String> {
        if cancelled() {
            return Err("操作已取消。".into());
        }
        let client = gateway::http_client().map_err(|e| e.to_string())?;
        let reserved = gateway::reserve_port(config, last, &client).map_err(|e| e.to_string())?;
        let port = reserved.local_addr().map_err(|e| e.to_string())?.port();
        drop(reserved);
        let mut g = Gateway::spawn(&self.exe, &self.paths, config, port, key.clone())?;
        g.wait_ready_until(&client, Duration::from_secs(90), cancelled)?;
        if cancelled() {
            return Err("操作已取消。".into());
        }
        Ok(g)
    }
    pub fn start(
        &mut self,
        key: &Zeroizing<String>,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), String> {
        let g = self.launch(&self.saved.settings, self.saved.port, key, &cancelled)?;
        let next = State {
            settings: self.saved.settings.clone(),
            port: Some(g.port),
        };
        settings::save_state(&self.paths.desktop, &next).map_err(|e| e.to_string())?;
        self.saved = next;
        self.gateway = Some(g);
        Ok(())
    }
    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(mut g) = self.gateway.take() {
            g.stop(Duration::from_secs(40))?;
        }
        Ok(())
    }
    /// Persist only after readiness. If starting or persisting the replacement
    /// fails, restore the previous listener and leave the previous settings intact.
    pub fn apply(
        &mut self,
        config: Settings,
        key: &Zeroizing<String>,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), String> {
        config.validate()?;
        if cancelled() {
            return Err("操作已取消。".into());
        }
        let old = self.saved.clone();
        self.stop()?;
        let attempt: Result<(), String> = (|| {
            // An explicit save prioritizes the requested port, not the previous
            // automatic choice; ordinary launches reuse the last selected port.
            let g = self.launch(&config, None, key, &cancelled)?;
            let next = State {
                settings: config,
                port: Some(g.port),
            };
            settings::save_state(&self.paths.desktop, &next)
                .map_err(|e| format!("保存设置失败：{e}"))?;
            self.saved = next;
            self.gateway = Some(g);
            Ok(())
        })();
        if let Err(error) = attempt {
            if cancelled() {
                return Err(error);
            }
            match self.launch(&old.settings, old.port, key, &cancelled) {
                Ok(g) => {
                    self.saved = old;
                    self.saved.port = Some(g.port);
                    self.gateway = Some(g);
                    // Original file remains intact if its replacement cannot be
                    // written. In-memory state still reports the actual listener.
                    let persistence = settings::save_state(&self.paths.desktop, &self.saved).err();
                    let suffix = if persistence.is_some() {
                        " 当前状态无法写入磁盘，请检查数据目录权限。"
                    } else {
                        ""
                    };
                    return Err(format!("{error}\n已恢复之前的监听设置。{suffix}"));
                }
                Err(restore) => {
                    return Err(format!(
                        "{error}\n恢复之前的监听设置也失败：{restore}\n请修正设置后重试。"
                    ))
                }
            }
        }
        Ok(())
    }
}
