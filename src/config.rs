use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "salita", about = "A personal home server")]
pub struct Cli {
    /// Path to config file
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Host to bind to
    #[arg(long)]
    pub host: Option<String>,

    /// Port to bind to
    #[arg(short, long)]
    pub port: Option<u16>,

    /// Site URL for WebAuthn (e.g. http://salita.local:6969)
    #[arg(long)]
    pub site_url: Option<String>,

    /// Path to data directory
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub storage: StorageConfig,
    pub auth: AuthConfig,
    pub tls: TlsConfig,
    pub instance_name: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub site_url: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct DatabaseConfig {
    pub path: Option<PathBuf>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct StorageConfig {
    pub path: Option<PathBuf>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct AuthConfig {
    pub cookie_name: String,
    pub session_hours: u64,
    /// Disable localhost authentication bypass (force auth even on localhost)
    /// Useful for testing or production environments that want strict auth
    pub disable_localhost_bypass: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct TlsConfig {
    pub enabled: bool,
    pub http_port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 6969,
            site_url: None,
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            cookie_name: "salita_session".to_string(),
            session_hours: 720,
            disable_localhost_bypass: false,
        }
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            http_port: 6970,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            storage: StorageConfig::default(),
            auth: AuthConfig::default(),
            tls: TlsConfig::default(),
            instance_name: "salita".to_string(),
        }
    }
}

impl Config {
    pub fn load(cli: &Cli) -> anyhow::Result<Self> {
        let data_dir = Self::data_dir(cli);
        let config_path = cli
            .config
            .clone()
            .unwrap_or_else(|| data_dir.join("config.toml"));

        let mut config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            toml::from_str(&content)?
        } else {
            Config::default()
        };

        // CLI overrides
        if let Some(ref host) = cli.host {
            config.server.host = host.clone();
        }
        if let Some(port) = cli.port {
            config.server.port = port;
        }
        if let Some(ref site_url) = cli.site_url {
            config.server.site_url = Some(site_url.clone());
        }

        // Resolve paths relative to data dir
        if config.database.path.is_none() {
            config.database.path = Some(data_dir.join("salita.db"));
        }
        if config.storage.path.is_none() {
            config.storage.path = Some(data_dir.join("uploads"));
        }

        Ok(config)
    }

    pub fn data_dir(cli: &Cli) -> PathBuf {
        cli.data_dir.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .expect("Could not determine home directory")
                .join(".salita")
        })
    }

    pub fn db_path(&self) -> &PathBuf {
        self.database.path.as_ref().unwrap()
    }

    pub fn uploads_path(&self) -> &PathBuf {
        self.storage.path.as_ref().unwrap()
    }

    pub fn site_url(&self) -> String {
        self.server
            .site_url
            .clone()
            .unwrap_or_else(|| crate::auth::webauthn::default_site_url(self.server.port))
    }

    pub fn tls_enabled(&self) -> bool {
        self.tls.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = Config::default();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 6969);
        assert_eq!(config.auth.cookie_name, "salita_session");
        assert_eq!(config.auth.session_hours, 720);
        assert!(config.database.path.is_none());
        assert!(config.storage.path.is_none());
    }

    #[test]
    fn data_dir_uses_cli_override() {
        let cli = Cli {
            config: None,
            host: None,
            port: None,
            site_url: None,
            data_dir: Some(PathBuf::from("/tmp/test-salita")),
        };
        assert_eq!(Config::data_dir(&cli), PathBuf::from("/tmp/test-salita"));
    }

    #[test]
    fn data_dir_defaults_to_home_dot_salita() {
        let cli = Cli {
            config: None,
            host: None,
            port: None,
            site_url: None,
            data_dir: None,
        };
        let dir = Config::data_dir(&cli);
        assert!(dir.ends_with(".salita"));
    }

    #[test]
    fn load_with_no_config_file_uses_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            config: None,
            host: None,
            port: None,
            site_url: None,
            data_dir: Some(tmp.path().to_path_buf()),
        };
        let config = Config::load(&cli).unwrap();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 6969);
        assert_eq!(config.db_path(), &tmp.path().join("salita.db"));
        assert_eq!(config.uploads_path(), &tmp.path().join("uploads"));
    }

    #[test]
    fn load_applies_cli_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli {
            config: None,
            host: Some("127.0.0.1".to_string()),
            port: Some(8080),
            site_url: None,
            data_dir: Some(tmp.path().to_path_buf()),
        };
        let config = Config::load(&cli).unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
    }

    #[test]
    fn load_reads_toml_file() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"
[server]
host = "192.168.1.1"
port = 9000

[auth]
cookie_name = "my_cookie"
session_hours = 24
"#,
        )
        .unwrap();

        let cli = Cli {
            config: Some(config_path),
            host: None,
            port: None,
            site_url: None,
            data_dir: Some(tmp.path().to_path_buf()),
        };
        let config = Config::load(&cli).unwrap();
        assert_eq!(config.server.host, "192.168.1.1");
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.auth.cookie_name, "my_cookie");
        assert_eq!(config.auth.session_hours, 24);
    }

    #[test]
    fn cli_overrides_beat_toml_values() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"
[server]
host = "192.168.1.1"
port = 9000
"#,
        )
        .unwrap();

        let cli = Cli {
            config: Some(config_path),
            host: Some("10.0.0.1".to_string()),
            port: Some(4000),
            site_url: None,
            data_dir: Some(tmp.path().to_path_buf()),
        };
        let config = Config::load(&cli).unwrap();
        assert_eq!(config.server.host, "10.0.0.1");
        assert_eq!(config.server.port, 4000);
    }

    #[test]
    fn site_url_defaults_to_localhost() {
        let config = Config::default();
        assert_eq!(config.site_url(), "http://localhost:6969");
    }

    #[test]
    fn site_url_uses_configured_value() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"
[server]
site_url = "http://salita.local:6969"
"#,
        )
        .unwrap();

        let cli = Cli {
            config: Some(config_path),
            host: None,
            port: None,
            site_url: None,
            data_dir: Some(tmp.path().to_path_buf()),
        };
        let config = Config::load(&cli).unwrap();
        assert_eq!(config.site_url(), "http://salita.local:6969");
    }

    #[test]
    fn site_url_cli_overrides_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"
[server]
site_url = "http://salita.local:6969"
"#,
        )
        .unwrap();

        let cli = Cli {
            config: Some(config_path),
            host: None,
            port: None,
            site_url: Some("http://myserver.local:8080".to_string()),
            data_dir: Some(tmp.path().to_path_buf()),
        };
        let config = Config::load(&cli).unwrap();
        assert_eq!(config.site_url(), "http://myserver.local:8080");
    }
}
