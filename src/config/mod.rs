//! Runtime configuration. Composed from a TOML file plus environment
//! overrides. The defaults are intentionally restrictive (loopback bind,
//! Bearer required for HTTP) so an out-of-the-box install is safe.

mod defaults;
mod model;
mod resolver;

pub use defaults::detect_cdb;
pub use model::{AuditConfig, AuthConfig, Config, DebuggerConfig, ServerConfig};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrip() {
        let cfg = Config::default();
        let s = toml::to_string(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.server.bind, cfg.server.bind);
        assert_eq!(
            back.debugger.attach_timeout_ms,
            cfg.debugger.attach_timeout_ms
        );
    }

    #[test]
    fn resolved_token_inline() {
        let mut cfg = Config::default();
        cfg.auth.bearer_token = Some("hello".into());
        assert_eq!(cfg.resolved_token().as_deref(), Some("hello"));
    }

    #[test]
    fn resolved_allowed_hosts_default_includes_loopback_and_bind() {
        let cfg = ServerConfig {
            bind: "192.168.1.10:8765".into(),
            ..Default::default()
        };
        let hosts = cfg.resolved_allowed_hosts();
        assert!(hosts.contains(&"localhost".to_string()));
        assert!(hosts.contains(&"127.0.0.1".to_string()));
        assert!(hosts.contains(&"::1".to_string()));
        assert!(hosts.contains(&"192.168.1.10".to_string()));
    }

    #[test]
    fn resolved_allowed_hosts_skips_wildcard_bind() {
        let cfg = ServerConfig {
            bind: "0.0.0.0:8765".into(),
            ..Default::default()
        };
        let hosts = cfg.resolved_allowed_hosts();
        assert!(!hosts.contains(&"0.0.0.0".to_string()));
    }

    #[test]
    fn resolved_allowed_hosts_explicit_overrides() {
        let mut cfg = ServerConfig {
            allowed_hosts: Some(vec!["only.example.com".into()]),
            ..Default::default()
        };
        assert_eq!(cfg.resolved_allowed_hosts(), vec!["only.example.com"]);
        cfg.allowed_hosts = Some(Vec::new());
        assert!(cfg.resolved_allowed_hosts().is_empty());
    }
}
