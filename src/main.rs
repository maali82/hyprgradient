mod config;
mod render;
mod wallpaper;

use wallpaper::WaylandWallpaper;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cfg = config::Config::load()?;
    let gradients = cfg.active_gradients()?;

    let (wallpaper, event_queue) = WaylandWallpaper::new(
        gradients,
        cfg.cycle_interval(),
        cfg.texture_resolution(),
    )?;

    wallpaper.run(event_queue)
}
