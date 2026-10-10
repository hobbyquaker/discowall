//! `/etc/discowall/discowall.toml`.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The interfaces discowall forwards between, by name. An interface that does not exist yet is
    /// waited for.
    pub interfaces: Vec<String>,
    #[serde(default = "yes")]
    pub ipv4: bool,
    #[serde(default = "yes")]
    pub ipv6: bool,
    #[serde(default = "default_socket")]
    pub control_socket: PathBuf,
    /// The rules file (`discoctl save` writes it, `discoctl apply` reloads it). Missing: an empty
    /// chain with the default policy `drop`.
    #[serde(default = "default_rules")]
    pub rules_file: PathBuf,
    /// A packet identical to one seen within this window is dropped (a second reflector, a bridge
    /// that delivers twice). mDNS repeats announcements no faster than once per second.
    #[serde(default = "default_dedup")]
    pub dedup_window_ms: u64,
}

fn yes() -> bool {
    true
}
fn default_socket() -> PathBuf {
    PathBuf::from(discowall_proto::DEFAULT_SOCKET)
}
fn default_rules() -> PathBuf {
    PathBuf::from("/etc/discowall/rules.toml")
}
fn default_dedup() -> u64 {
    500
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        if self.interfaces.len() < 2 {
            bail!(
                "at least two interfaces are needed to forward between (got {})",
                self.interfaces.len()
            );
        }
        let mut seen = std::collections::HashSet::new();
        for name in &self.interfaces {
            if name.is_empty() || name.len() >= 16 || name.contains(['/', ' ']) {
                bail!("not an interface name: {name:?}");
            }
            if !seen.insert(name) {
                bail!("interface {name} listed twice");
            }
        }
        if !self.ipv4 && !self.ipv6 {
            bail!("ipv4 and ipv6 are both off");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal() {
        let c: Config = toml::from_str("interfaces = [\"lan\", \"iot\"]").unwrap();
        c.validate().unwrap();
        assert!(c.ipv4 && c.ipv6);
        assert_eq!(c.dedup_window_ms, 500);
        assert_eq!(
            c.control_socket,
            PathBuf::from("/run/discowall/control.sock")
        );
    }

    #[test]
    fn rejects() {
        let one: Config = toml::from_str("interfaces = [\"lan\"]").unwrap();
        assert!(one.validate().is_err());
        let dup: Config = toml::from_str("interfaces = [\"lan\", \"lan\"]").unwrap();
        assert!(dup.validate().is_err());
        let off: Config =
            toml::from_str("interfaces = [\"a\", \"b\"]\nipv4 = false\nipv6 = false").unwrap();
        assert!(off.validate().is_err());
        assert!(toml::from_str::<Config>("interfaces = [\"a\", \"b\"]\nbogus = 1").is_err());
    }
}
