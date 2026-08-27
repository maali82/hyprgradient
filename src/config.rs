use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct Stop {
    pub position: u8,
    pub color: [u8; 3],
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum GradientType {
    Linear,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GradientProfile {
    pub name: String,
    pub active: bool,
    #[serde(rename = "type")]
    pub gradient_type: GradientType,
    pub direction: f32,
    pub stops: Vec<Stop>,
}

fn default_log_level() -> String { "info".to_string() }
fn default_monitor() -> String { "all".to_string() }
fn default_texture_resolution() -> u32 { 256 }

#[derive(Debug, Deserialize)]
pub struct Settings {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_texture_resolution")]
    pub texture_resolution: u32,
    pub cycle_interval: u64,
    #[serde(default = "default_monitor")]
    pub monitor: String,
    pub random: bool,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub settings: Settings,
    pub gradients: Vec<GradientProfile>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::default_path()?;

        if !path.exists() {
            Self::write_config_file(&path)?;
        }

        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config file at {}", path.display()))?;

        let cfg: Config = toml::from_str(&text)
            .with_context(|| format!("parsing config file at {}", path.display()))?;

        cfg.validate()?;
        Ok(cfg)
    }

    fn write_config_file(path: &Path) -> Result<()> {
        let config = include_str!("config/hyprgradient.conf");

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating config directory {}", parent.display()))?;
        }

        std::fs::write(path, config)
            .with_context(|| format!("writing config file at {}", path.display()))?;

        Ok(())
    }

    fn default_path() -> Result<PathBuf> {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            return Ok(PathBuf::from(xdg).join("hypr/hyprgradient.conf"));
        }

        let home = std::env::var("HOME").context("HOME is not set")?;
        Ok(PathBuf::from(home).join(".config/hypr/hyprgradient.conf"))
    }

    fn validate(&self) -> Result<()> {
        if self.settings.texture_resolution == 0 {
            bail!("texture_resolution must be greater than zero");
        }
        if self.gradients.is_empty() {
            bail!("config contains no gradients");
        }
        for gradient in &self.gradients {
            if gradient.name.trim().is_empty() {
                bail!("gradient has an empty name");
            }
            if gradient.stops.is_empty() {
                bail!("gradient `{}` has no color stops", gradient.name);
            }
            if !gradient.direction.is_finite() {
                bail!("gradient `{}` has an invalid direction", gradient.name);
            }
            for stop in &gradient.stops {
                if stop.position > 100 {
                    bail!("gradient `{}` has invalid stop position {}", gradient.name, stop.position);
                }
            }
        }
        Ok(())
    }

    pub fn cycle_interval(&self) -> Duration { Duration::from_secs(self.settings.cycle_interval) }

    pub fn texture_resolution(&self) -> u32 { self.settings.texture_resolution }

    pub fn active_gradients(&self) -> Result<Vec<GradientProfile>> {
        let mut pool: Vec<_> = self.gradients.iter().filter(|g| g.active).cloned().collect();
        if pool.is_empty() {
            bail!("no gradient in the config has `active = true`");
        }
        for gradient in &mut pool {
            gradient.stops.sort_by_key(|stop| stop.position);
        }
        Ok(pool)
    }
}
