mod config;
mod render;
mod wallpaper;
mod cli;

use wallpaper::WaylandWallpaper;
use clap::Parser;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cli = cli:Cli::parse();

    let cfg = config::Config::load()?;
    let gradients = cfg.active_gradients()?;

    // because we need cfg and gradient loaded for these
    // but... argh, need like an IPC with .sock for this
    // need to think about this.
    match cli.command {
        Some(Commands::Halt) => {
            // might upset someone in ~560 million years that it's not really stopping the timer :P
            cfg.cycle_interval = u64::MAX;
            todo!();
        }
        Some(Commands::Next) => {todo!();}
        Some(Commands::Reload) => {todo!();}
        Some(Commands::Load { name }) => {todo!();}
        None => {}
    }


    let (wallpaper, event_queue) = WaylandWallpaper::new(
        gradients,
        cfg.cycle_interval(),
        cfg.texture_resolution(),
    )?;

    wallpaper.run(event_queue)
}
