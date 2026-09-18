use crate::bail;
use crate::file_ops::FileOps;
use rand::{seq::SliceRandom, RngExt};
use serde::Deserialize;
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

fn default_monitor() -> String {
    "all".to_string()
}
fn default_texture_resolution() -> u32 {
    256
}

#[derive(Debug, Deserialize)]
pub struct Settings {
    #[serde(default = "default_texture_resolution")]
    pub texture_resolution: u32,
    pub cycle_interval: u64,
    #[serde(default = "default_monitor")]
    pub monitor: String,
    pub randomize: bool,
    pub random_direction: bool,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub settings: Settings,
    pub gradients: Vec<GradientProfile>,
}

impl Config {
    pub fn load() -> Self {
        let path = FileOps::config_dir().join(crate::CONFIG_FILE);

        if !path.exists() {
            let default_config = include_str!("config/hyprgradient.conf");
            FileOps::write_string(&path, default_config);
        }

        let text = FileOps::read_to_string(&path);

        let cfg: Config = match toml::from_str(&text) {
            Ok(cfg) => cfg,
            Err(error) => bail!("parsing config file at {}: {error}", path.display()),
        };

        cfg.validate();
        cfg
    }

    fn validate(&self) {
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
                    bail!(
                        "gradient `{}` has invalid stop position {}",
                        gradient.name,
                        stop.position
                    );
                }
            }
        }
    }

    pub fn cycle_interval(&self) -> Duration {
        Duration::from_secs(self.settings.cycle_interval)
    }

    pub fn texture_resolution(&self) -> u32 {
        self.settings.texture_resolution
    }

    pub fn active_gradients(&self) -> Vec<GradientProfile> {
        let mut pool: Vec<_> = self
            .gradients
            .iter()
            .filter(|g| g.active)
            .cloned()
            .collect();
        if pool.is_empty() {
            bail!("no gradient in the config has `active = true`");
        }

        if self.settings.randomize {
            pool.shuffle(&mut rand::rng());
        }

        for gradient in &mut pool {
            gradient.stops.sort_by_key(|stop| stop.position);

            if self.settings.random_direction {
                gradient.direction = rand::rng().random_range(0.0..359.9);
            }
        }
        pool
    }
}
