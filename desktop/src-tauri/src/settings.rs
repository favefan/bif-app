// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::{fs, io, net::Ipv4Addr, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub host: String,
    pub preferred_port: u16,
    pub auto_port: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            preferred_port: 8080,
            auto_port: true,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.host.as_str(), "127.0.0.1" | "0.0.0.0") {
            return Err("监听地址必须为 127.0.0.1（仅本机）或 0.0.0.0（所有 IPv4 网卡）。".into());
        }
        if self.preferred_port == 0 {
            return Err("端口必须为 1–65535。".into());
        }
        Ok(())
    }
    pub fn address(&self) -> Ipv4Addr {
        if self.host == "0.0.0.0" {
            Ipv4Addr::UNSPECIFIED
        } else {
            Ipv4Addr::LOCALHOST
        }
    }
    pub fn ports(&self, last_port: Option<u16>) -> Vec<u16> {
        let mut ports = Vec::new();
        if self.auto_port {
            if let Some(last) = last_port.filter(|p| *p > 0) {
                ports.push(last);
            }
        }
        ports.push(self.preferred_port);
        if self.auto_port {
            for delta in 1..=100u32 {
                let port = (u32::from(self.preferred_port) - 1 + delta) % 65535 + 1;
                if !ports.contains(&(port as u16)) {
                    ports.push(port as u16);
                }
            }
        }
        ports
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(flatten)]
    pub settings: Settings,
    pub port: Option<u16>,
}

pub fn read_state(dir: &Path) -> io::Result<State> {
    let bytes = match fs::read(dir.join("state.json")) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(State::default()),
        Err(e) => return Err(e),
    };
    let mut raw: serde_json::Value = serde_json::from_slice(&bytes)?;
    // Alpha.1 stored only the selected port. Preserve an explicitly edited port
    // as the preference when migrating; no Bifrost configuration is copied.
    if raw.get("preferred_port").is_none() {
        if let Some(port) = raw
            .get("port")
            .and_then(|p| p.as_u64())
            .filter(|p| (1..=65535).contains(p))
        {
            raw["preferred_port"] = port.into();
        }
    }
    let state: State = serde_json::from_value(raw)?;
    state.settings.validate().map_err(io::Error::other)?;
    Ok(state)
}
pub fn save_state(dir: &Path, state: &State) -> io::Result<()> {
    state.settings.validate().map_err(io::Error::other)?;
    let temp = dir.join("state.json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(state)?)?;
    fs::rename(temp, dir.join("state.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_bind_address_port_and_fallback_boundaries() {
        assert!(Settings::default().validate().is_ok());
        assert!(Settings {
            host: "example.com".into(),
            ..Settings::default()
        }
        .validate()
        .is_err());
        assert!(Settings {
            preferred_port: 0,
            ..Settings::default()
        }
        .validate()
        .is_err());
        let s = Settings {
            preferred_port: 65535,
            ..Settings::default()
        };
        assert_eq!(s.ports(None)[..3], [65535, 1, 2]);
        assert!(!s.ports(None).contains(&0));
        assert_eq!(
            Settings {
                auto_port: false,
                ..s
            }
            .ports(Some(9000)),
            [65535]
        );
    }
    #[test]
    fn migrates_legacy_state_and_keeps_preference_separate() {
        let dir =
            std::env::temp_dir().join(format!("bif-app-settings-{}", crate::platform::test_id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("state.json"), br#"{"port":8088}"#).unwrap();
        let mut s = read_state(&dir).unwrap();
        assert_eq!(s.settings.preferred_port, 8088);
        assert_eq!(s.settings.host, "127.0.0.1");
        s.port = Some(8090);
        save_state(&dir, &s).unwrap();
        let persisted = read_state(&dir).unwrap();
        assert_eq!(persisted.settings.preferred_port, 8088);
        assert_eq!(persisted.port, Some(8090));
        fs::write(dir.join("state.json"), br#"{"host":"example.com"}"#).unwrap();
        assert!(read_state(&dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
