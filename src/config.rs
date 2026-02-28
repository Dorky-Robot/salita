use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "salita", about = "A home device mesh with MCP interface")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Path to config file
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Path to data directory
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the HTTP daemon with mDNS discovery
    Serve {
        /// Host to bind to
        #[arg(long)]
        host: Option<String>,

        /// Port to bind to
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Run the MCP stdio server
    Mcp,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub directories: Vec<DirectoryConfig>,
    pub max_read_bytes: usize,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Deserialize, Debug, Clone)]
pub struct DirectoryConfig {
    pub label: String,
    pub path: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 6969,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            directories: Vec::new(),
            max_read_bytes: 10 * 1024 * 1024, // 10MB
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

        let mut config: Config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            toml::from_str(&content)?
        } else {
            Config::default()
        };

        // CLI overrides for serve command
        if let Command::Serve { ref host, ref port } = cli.command {
            if let Some(ref h) = host {
                config.server.host = h.clone();
            }
            if let Some(p) = port {
                config.server.port = *p;
            }
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

    pub fn db_path(cli: &Cli) -> PathBuf {
        Self::data_dir(cli).join("salita.db")
    }

    /// Resolve a directory label to its expanded path
    pub fn resolve_directory(&self, label: &str) -> Option<PathBuf> {
        self.directories
            .iter()
            .find(|d| d.label == label)
            .map(|d| expand_tilde(&d.path))
    }
}

/// Expand ~ to the user's home directory
fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            return home.join(path.strip_prefix("~/").unwrap_or(&path[1..]));
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = Config::default();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 6969);
        assert_eq!(config.max_read_bytes, 10 * 1024 * 1024);
        assert!(config.directories.is_empty());
    }

    #[test]
    fn expand_tilde_works() {
        let expanded = expand_tilde("~/Documents");
        assert!(!expanded.to_string_lossy().starts_with('~'));
        assert!(expanded.to_string_lossy().ends_with("Documents"));
    }

    #[test]
    fn expand_tilde_absolute_passthrough() {
        let expanded = expand_tilde("/tmp/test");
        assert_eq!(expanded, PathBuf::from("/tmp/test"));
    }

    #[test]
    fn resolve_directory_finds_label() {
        let config = Config {
            directories: vec![DirectoryConfig {
                label: "docs".to_string(),
                path: "/tmp/docs".to_string(),
            }],
            ..Config::default()
        };
        assert_eq!(
            config.resolve_directory("docs"),
            Some(PathBuf::from("/tmp/docs"))
        );
        assert_eq!(config.resolve_directory("nonexistent"), None);
    }
}
