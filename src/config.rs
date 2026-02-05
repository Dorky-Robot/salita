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

    /// Path to data directory
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub storage: StorageConfig,
    pub auth: AuthConfig,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            cookie_name: "salita_session".to_string(),
            session_hours: 720,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = Config::default();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 3000);
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
            data_dir: Some(tmp.path().to_path_buf()),
        };
        let config = Config::load(&cli).unwrap();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 3000);
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
            data_dir: Some(tmp.path().to_path_buf()),
        };
        let config = Config::load(&cli).unwrap();
        assert_eq!(config.server.host, "10.0.0.1");
        assert_eq!(config.server.port, 4000);
    }
}
