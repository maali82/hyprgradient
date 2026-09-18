mod config;
mod file_ops;
mod file_watcher;
mod log;
mod render;
mod wallpaper;

use smithay_client_toolkit::reexports::calloop::EventLoop;
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use config::{Config, GradientProfile};
use file_ops::FileOps;
use file_watcher::FileWatcher;
use wallpaper::WaylandWallpaper;

/// File names live here, not in `files.rs` - `files.rs` only knows *how*
/// to read/write/watch files, not *which* files this app cares about.
pub const CONFIG_FILE: &str = "hyprgradient.conf";
pub const EVENT_FILE: &str = ".hyprgradientevent";

fn run() {
    let mut config = Config::load();
    let mut gradients = config.active_gradients();
    let mut current_index = 0usize;

    let (mut wallpaper, event_queue) = WaylandWallpaper::new(config.texture_resolution());
    let mut event_loop: EventLoop<WaylandWallpaper> = match EventLoop::try_new() {
        Ok(event_loop) => event_loop,
        Err(error) => bail!("creating event loop: {error}"),
    };

    wallpaper.insert_wayland_source(&event_loop, event_queue);

    set_current_gradient(&mut wallpaper, &gradients, current_index);

    let (event_tx, event_rx) = mpsc::channel::<String>();

    thread::spawn(move || {
        let directory = FileOps::ensure_config_dir();
        let event_file = directory.join(EVENT_FILE);
        let watcher = FileWatcher::new(directory, event_file);

        loop {
            let event = watcher.wait_for_event();
            if event_tx.send(event).is_err() {
                break;
            }
        }
    });

    let mut interval = config.cycle_interval();
    let mut next_tick = Instant::now() + interval;

    let mut running = true;

    while running {
        let timeout = next_tick.saturating_duration_since(Instant::now());
        if let Err(error) = event_loop.dispatch(Some(timeout), &mut wallpaper) {
            bail!("dispatching wayland events: {error}");
        }

        while let Ok(event) = event_rx.try_recv() {
            match event.as_str() {
                "next" => next_event(
                    &mut wallpaper,
                    &gradients,
                    &mut current_index,
                    &mut next_tick,
                    interval,
                ),
                "halt" => halt_event(&mut running),
                "reload" => reload_event(
                    &mut wallpaper,
                    &mut config,
                    &mut gradients,
                    &mut current_index,
                    &mut interval,
                    &mut next_tick,
                ),
                other => bail!("unknown event: {other:?}"),
            }
        }

        if gradients.len() > 1 && Instant::now() >= next_tick {
            advance_gradient(&mut wallpaper, &gradients, &mut current_index);
            next_tick = Instant::now() + interval;
        }
    }
}

fn halt_event(running: &mut bool) {
    log!("received halt event");
    *running = false;
}

fn next_event(
    wallpaper: &mut WaylandWallpaper,
    gradients: &[GradientProfile],
    current_index: &mut usize,
    next_tick: &mut Instant,
    interval: Duration,
) {
    log!("received next event");
    advance_gradient(wallpaper, gradients, current_index);
    *next_tick = Instant::now() + interval;
}

fn reload_event(
    wallpaper: &mut WaylandWallpaper,
    config: &mut Config,
    gradients: &mut Vec<GradientProfile>,
    current_index: &mut usize,
    interval: &mut Duration,
    next_tick: &mut Instant,
) {
    log!("received reload event");
    *config = Config::load();
    *interval = config.cycle_interval();
    *gradients = config.active_gradients();
    *current_index = 0;
    set_current_gradient(wallpaper, gradients, *current_index);
    *next_tick = Instant::now() + *interval;
}

fn set_current_gradient(
    wallpaper: &mut WaylandWallpaper,
    gradients: &[GradientProfile],
    index: usize,
) {
    let Some(gradient) = gradients.get(index).cloned() else {
        bail!("no gradient available");
    };
    wallpaper.set_gradient(gradient);
}

fn advance_gradient(
    wallpaper: &mut WaylandWallpaper,
    gradients: &[GradientProfile],
    current_index: &mut usize,
) {
    if gradients.len() <= 1 {
        return;
    }

    *current_index = (*current_index + 1) % gradients.len();
    set_current_gradient(wallpaper, gradients, *current_index);
}

/// Write an event word to the event file, for an already-running instance to pick up.
fn send_event(word: &str) {
    let directory = FileOps::ensure_config_dir();
    let path = directory.join(EVENT_FILE);
    FileOps::write_string(&path, word);
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("halt") => send_event("halt"),
        Some("next") => send_event("next"),
        Some("reload") => send_event("reload"),
        Some(other) => bail!("unknown command: {other}"),
        None => run(),
    }
}
