//! `profile.ron`: what this player keeps between games (see
//! [`crate::sim::progression`]).
//!
//! Loaded at startup, sent to the host on joining (`net::profiles`), and
//! saved whenever what the host sends back changes, and on quitting. A
//! missing file starts a fresh profile; an unreadable one is kept aside as
//! `profile.ron.bad` and a fresh one started, rather than lost or fatal.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::sim::progression::ProfileFile;

pub const PROFILE_PATH: &str = "profile.ron";

/// This machine's profile: where it's kept, and what was last loaded or
/// saved.
#[derive(Resource, Debug)]
pub struct ProfileStore {
    pub path: PathBuf,
    /// What's in the file.
    pub file: ProfileFile,
    /// Sent to the host already (this session).
    pub uploaded: bool,
    /// Seconds since it was last written.
    pub since_save: f32,
    /// Seconds the host's copy has been here.
    pub held_for: f32,
}

impl ProfileStore {
    /// The store at `path`, with what's there (see [`load`]).
    pub fn open(path: impl Into<PathBuf>) -> ProfileStore {
        let path = path.into();
        ProfileStore {
            file: load(&path),
            path,
            uploaded: false,
            since_save: 0.0,
            held_for: 0.0,
        }
    }

    /// Writes `file` if it's not what's there already. Whether it wrote.
    pub fn save(&mut self, file: ProfileFile) -> bool {
        if file == self.file {
            return false;
        }
        match write(&self.path, &file) {
            Ok(()) => {
                self.file = file;
                self.since_save = 0.0;
                true
            }
            Err(e) => {
                warn!("couldn't save {}: {e}", self.path.display());
                false
            }
        }
    }
}

/// The profile at `path`: fresh if there's none; fresh if it can't be read,
/// with the unreadable file kept beside it as `<path>.bad`.
pub fn load(path: &Path) -> ProfileFile {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!("no {}: a fresh profile", path.display());
            return ProfileFile::fresh();
        }
        Err(e) => {
            warn!("couldn't read {}: {e}; a fresh profile", path.display());
            return ProfileFile::fresh();
        }
    };
    match ron::from_str::<ProfileFile>(&text) {
        Ok(file) => {
            info!("profile from {}", path.display());
            file
        }
        Err(e) => {
            let mut bad = path.as_os_str().to_owned();
            bad.push(".bad");
            warn!("{} is unreadable ({e}); kept as {}, starting fresh", path.display(), PathBuf::from(&bad).display());
            if let Err(e) = std::fs::rename(path, &bad) {
                warn!("couldn't keep it aside: {e}");
            }
            ProfileFile::fresh()
        }
    }
}

fn write(path: &Path, file: &ProfileFile) -> std::io::Result<()> {
    let text = ron::ser::to_string_pretty(file, ron::ser::PrettyConfig::default()).map_err(std::io::Error::other)?;
    // Written whole, then moved into place, so a crash mid-write can't
    // leave half a profile.
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Startup: loads `profile.ron` (unless something put a store in already).
pub fn load_profile(store: Option<Res<ProfileStore>>, mut commands: Commands) {
    if store.is_none() {
        commands.insert_resource(ProfileStore::open(PROFILE_PATH));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hullbreakers-save-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("profile.ron")
    }

    #[test]
    fn a_missing_profile_starts_fresh_and_saves() {
        let path = temp("missing");
        let _ = std::fs::remove_file(&path);
        let mut store = ProfileStore::open(&path);
        assert_eq!(store.file, ProfileFile::fresh());
        let mut file = ProfileFile::fresh();
        file.profile.salvage = 77;
        assert!(store.save(file.clone()));
        assert!(!store.save(file.clone()), "wrote it again unchanged");
        assert_eq!(load(&path), file);
    }

    #[test]
    fn an_unreadable_profile_is_kept_aside() {
        let path = temp("bad");
        std::fs::write(&path, "(version: 1, profile: (stash: [nonsense").unwrap();
        let file = load(&path);
        assert_eq!(file, ProfileFile::fresh());
        let mut bad = path.as_os_str().to_owned();
        bad.push(".bad");
        assert!(std::fs::read_to_string(PathBuf::from(bad)).unwrap().contains("nonsense"));
        assert!(!path.exists());
    }
}
