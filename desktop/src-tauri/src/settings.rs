// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::Path,
};

pub fn parse_host(host: &str) -> Result<IpAddr, String> {
    let host = host.trim();
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(Ipv4Addr::LOCALHOST.into());
    }
    host.parse::<IpAddr>()
        .map(|ip| ip.to_canonical())
        .map_err(|_| "请输入有效的 IP 地址或 localhost，不要包含协议、端口或路径。".into())
}

pub fn endpoint(address: IpAddr, port: u16) -> SocketAddr {
    let address = match address {
        IpAddr::V4(ip) if ip.is_unspecified() => Ipv4Addr::LOCALHOST.into(),
        IpAddr::V6(ip) if ip.is_unspecified() => Ipv6Addr::LOCALHOST.into(),
        ip => ip,
    };
    SocketAddr::new(address, port)
}

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
        parse_host(&self.host)?;
        if self.preferred_port == 0 {
            return Err("端口必须为 1–65535。".into());
        }
        Ok(())
    }
    pub fn address(&self) -> IpAddr {
        parse_host(&self.host).expect("Settings must be validated before use")
    }
    pub fn normalized(mut self) -> Result<Self, String> {
        self.validate()?;
        self.host = if self.host.trim().eq_ignore_ascii_case("localhost") {
            "localhost".into()
        } else {
            self.address().to_string()
        };
        Ok(self)
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
    fn custom_hosts_match_renderer_validation_cases() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/host-cases.json")).unwrap();
        for host in cases["valid"].as_array().unwrap() {
            assert!(parse_host(host.as_str().unwrap()).is_ok(), "{host}");
        }
        for host in cases["invalid"].as_array().unwrap() {
            assert!(parse_host(host.as_str().unwrap()).is_err(), "{host}");
        }
        assert_eq!(
            parse_host(" LOCALHOST ").unwrap(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
        for (host, expected) in [
            ("0.0.0.0", "127.0.0.1:8080"),
            ("192.168.1.42", "192.168.1.42:8080"),
            ("::", "[::1]:8080"),
            ("2001:db8::1", "[2001:db8::1]:8080"),
        ] {
            assert_eq!(
                endpoint(parse_host(host).unwrap(), 8080).to_string(),
                expected
            );
        }
    }
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
