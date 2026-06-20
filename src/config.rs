use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};

use serde::Deserialize;

const DEFAULT_LISTEN_HOST: &str = "0.0.0.0";
const DEFAULT_LISTEN_PORT: NonZeroU16 = NonZeroU16::new(28882).unwrap();
const DEFAULT_WEB_WORKERS: NonZeroUsize = NonZeroUsize::new(4).unwrap();
const DEFAULT_SCHEDULE_PATH: &str = "arkiv-protocol-schedule.json";
const DEFAULT_RPC_POLL_SECONDS: NonZeroU64 = NonZeroU64::new(10).unwrap();
const DEFAULT_RPC_TIMEOUT_MS: NonZeroU64 = NonZeroU64::new(5000).unwrap();

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen_host")]
    pub listen_host: String,
    #[serde(default = "default_listen_port")]
    pub listen_port: NonZeroU16,
    #[serde(default = "default_web_workers")]
    pub web_workers: NonZeroUsize,
    #[serde(default = "default_schedule_path")]
    pub schedule_path: String,
    #[serde(default)]
    pub chain_id: Option<u64>,
    #[serde(default)]
    pub rpc_url: Option<String>,
    #[serde(default = "default_rpc_poll_seconds")]
    pub rpc_poll_seconds: NonZeroU64,
    #[serde(default = "default_rpc_timeout_ms")]
    pub rpc_timeout_ms: NonZeroU64,
    #[serde(default)]
    pub admin_bearer_key: Option<String>,
}

pub fn create_config() -> Config {
    envy::from_env::<Config>().unwrap_or_else(|err| panic!("invalid config: {err}"))
}

fn default_listen_host() -> String {
    DEFAULT_LISTEN_HOST.to_string()
}

fn default_listen_port() -> NonZeroU16 {
    DEFAULT_LISTEN_PORT
}

fn default_web_workers() -> NonZeroUsize {
    DEFAULT_WEB_WORKERS
}

fn default_schedule_path() -> String {
    DEFAULT_SCHEDULE_PATH.to_string()
}

fn default_rpc_poll_seconds() -> NonZeroU64 {
    DEFAULT_RPC_POLL_SECONDS
}

fn default_rpc_timeout_ms() -> NonZeroU64 {
    DEFAULT_RPC_TIMEOUT_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_pairs<const N: usize>(pairs: [(&str, &str); N]) -> Result<Config, envy::Error> {
        envy::from_iter(pairs.into_iter().map(|(k, v)| (k.to_string(), v.to_string())))
    }

    #[test]
    fn defaults_apply_when_env_is_empty() {
        let config = from_pairs([]).unwrap();
        assert_eq!(config.listen_host, DEFAULT_LISTEN_HOST);
        assert_eq!(config.listen_port, DEFAULT_LISTEN_PORT);
        assert_eq!(config.web_workers, DEFAULT_WEB_WORKERS);
        assert_eq!(config.schedule_path, DEFAULT_SCHEDULE_PATH);
        assert_eq!(config.chain_id, None);
        assert_eq!(config.rpc_url, None);
        assert_eq!(config.rpc_poll_seconds, DEFAULT_RPC_POLL_SECONDS);
        assert_eq!(config.rpc_timeout_ms, DEFAULT_RPC_TIMEOUT_MS);
        assert_eq!(config.admin_bearer_key, None);
    }

    #[test]
    fn parses_valid_overrides() {
        let config = from_pairs([
            ("SCHEDULE_PATH", "/etc/arkiv/schedule.json"),
            ("CHAIN_ID", "42069"),
            ("RPC_URL", "http://localhost:8545"),
            ("RPC_POLL_SECONDS", "5"),
            ("ADMIN_BEARER_KEY", "s3cret"),
        ])
        .unwrap();
        assert_eq!(config.schedule_path, "/etc/arkiv/schedule.json");
        assert_eq!(config.chain_id, Some(42069));
        assert_eq!(config.rpc_url.as_deref(), Some("http://localhost:8545"));
        assert_eq!(config.rpc_poll_seconds.get(), 5);
        assert_eq!(config.admin_bearer_key.as_deref(), Some("s3cret"));
    }

    #[test]
    fn rejects_non_integer_poll_seconds() {
        assert!(from_pairs([("RPC_POLL_SECONDS", "abc")]).is_err());
    }

    #[test]
    fn rejects_zero_poll_seconds() {
        assert!(from_pairs([("RPC_POLL_SECONDS", "0")]).is_err());
    }
}
