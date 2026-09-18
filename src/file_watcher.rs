//! Watches the config directory for the `.hyprgradientevent` file and
//! hands back whatever it said. It doesn't know or care what the string
//! means - that's for `main.rs` to decide.

use std::path::PathBuf;

use inotify::{Inotify, WatchMask};

use crate::bail;
use crate::file_ops::FileOps;

pub struct FileWatcher {
    directory: PathBuf,
    event_file: PathBuf,
}

impl FileWatcher {
    pub fn new(directory: PathBuf, event_file: PathBuf) -> Self {
        Self {
            directory,
            event_file,
        }
    }

    /// Block until the event file appears, then read + remove it and
    /// return its contents.
    pub fn wait_for_event(&self) -> String {
        // Handle a file that was already sitting there before we start watching.
        if self.event_file.exists() {
            return self.read_event();
        }

        let mut inotify = match Inotify::init() {
            Ok(inotify) => inotify,
            Err(error) => bail!("initializing inotify: {error}"),
        };

        if let Err(error) = inotify
            .watches()
            .add(&self.directory, WatchMask::CREATE | WatchMask::MOVED_TO)
        {
            bail!("watching {}: {error}", self.directory.display());
        }

        let mut buffer = [0u8; 4096];

        loop {
            let events = match inotify.read_events_blocking(&mut buffer) {
                Ok(events) => events,
                Err(error) => bail!("reading inotify events: {error}"),
            };

            for event in events {
                let Some(name) = event.name else { continue };
                if name != self.event_file.file_name().unwrap() {
                    continue;
                }
                return self.read_event();
            }
        }
    }

    fn read_event(&self) -> String {
        let contents = FileOps::read_to_string(&self.event_file);
        FileOps::remove_file(&self.event_file);
        contents.trim().to_string()
    }
}
