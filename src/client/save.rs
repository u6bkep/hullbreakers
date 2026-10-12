//! `profile.ron`: what this player keeps between games (see
//! [`crate::sim::progression`]).
//!
//! Loaded at startup, sent to the host on joining (`net::profiles`), and
//! saved whenever the host's snapshot of it changes. A missing file starts
//! a fresh profile; an unreadable one is kept aside as `profile.ron.bad`
//! and a fresh one started, rather than lost or fatal. A file that exists
//! but can't be read at all (permissions, a disk error) starts a fresh
//! profile that is never saved, so the file is left as it was.
//!
//! `HULLBREAKERS_SAVE=<path>` keeps it somewhere else (the test harness
//! gives each game it runs its own, away from the player's).

use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::sim::progression::{ProfileFile, callsign};

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
    /// Whether saving is allowed (not when the file couldn't be read).
    pub writable: bool,
}

impl ProfileStore {
    /// The store at `path`, with what's there (see [`read`]).
    pub fn open(path: impl Into<PathBuf>) -> ProfileStore {
        let path = path.into();
        let (file, writable) = read(&path);
        let mut store = ProfileStore {
            file: file.clone(),
            path,
            uploaded: false,
            writable,
        };
        // A profile is someone: a new one (or one from before ids) gets an
        // id and a callsign, kept at once.
        let mut known = file;
        if known.profile.id == 0 {
            known.profile.id = rand::random::<u64>().max(1);
        }
        if known.profile.name.is_empty() {
            known.profile.name = callsign(known.profile.id);
        }
        if known != store.file {
            if !store.save(known.clone()) {
                store.file = known;
            }
        }
        store
    }

    /// Writes `file` if it's not what's there already. Whether it wrote.
    pub fn save(&mut self, file: ProfileFile) -> bool {
        if file == self.file || !self.writable {
            return false;
        }
        match write(&self.path, &file) {
            Ok(()) => {
                self.file = file;
                true
            }
            Err(e) => {
                warn!("couldn't save {}: {e}", self.path.display());
                false
            }
        }
    }
}

/// The profile at `path` (see [`read`]).
pub fn load(path: &Path) -> ProfileFile {
    read(path).0
}

/// The profile at `path`, and whether it may be written back: fresh (and
/// writable) if there's none; fresh if it can't be parsed, with the bad
/// file kept beside it (`<path>.bad`, or `.bad.2`, ... if that's taken);
/// fresh and not writable if it can't be read at all.
pub fn read(path: &Path) -> (ProfileFile, bool) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!("no {}: a fresh profile", path.display());
            return (ProfileFile::fresh(), true);
        }
        Err(e) => {
            warn!("couldn't read {}: {e}; a fresh profile, which won't be saved over it", path.display());
            return (ProfileFile::fresh(), false);
        }
    };
    match ron::from_str::<ProfileFile>(&text) {
        Ok(file) => {
            info!("profile from {}", path.display());
            (file, true)
        }
        Err(e) => {
            let bad = aside(path);
            warn!("{} is unreadable ({e}); kept as {}, starting fresh", path.display(), bad.display());
            match std::fs::rename(path, &bad) {
                Ok(()) => (ProfileFile::fresh(), true),
                Err(e) => {
                    warn!("couldn't keep it aside ({e}): not saving over it");
                    (ProfileFile::fresh(), false)
                }
            }
        }
    }
}

/// A name beside `path` for a bad file that doesn't overwrite an earlier one.
fn aside(path: &Path) -> PathBuf {
    let named = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    std::iter::once(named(".bad"))
        .chain((2..).map(|n| named(&format!(".bad.{n}"))))
        .find(|p| !p.exists())
        .expect("unbounded")
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

/// Startup: loads the profile (unless something put a store in already).
pub fn load_profile(store: Option<Res<ProfileStore>>, mut commands: Commands) {
    if store.is_none() {
        let path = std::env::var_os("HULLBREAKERS_SAVE").map_or(PathBuf::from(PROFILE_PATH), PathBuf::from);
        commands.insert_resource(ProfileStore::open(path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hullbreakers-save-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("profile.ron")
    }

    #[test]
    fn a_missing_profile_starts_fresh_and_saves() {
        let path = temp("missing");
        let mut store = ProfileStore::open(&path);
        // Fresh, but someone: an id and a callsign, kept at once.
        let id = store.file.profile.id;
        assert_ne!(id, 0);
        assert_eq!(store.file.profile.name, callsign(id));
        assert_eq!(load(&path), store.file);
        assert_eq!(ProfileStore::open(&path).file.profile.id, id, "a new id each time");
        let mut file = ProfileFile::fresh();
        file.profile.salvage = 77;
        assert!(store.save(file.clone()));
        assert!(!store.save(file.clone()), "wrote it again unchanged");
        assert_eq!(load(&path), file);
    }

    #[test]
    fn an_unreadable_profile_is_kept_aside_without_losing_an_earlier_one() {
        let path = temp("bad");
        for nonsense in ["first", "second"] {
            std::fs::write(&path, format!("(version: 1, profile: (stash: [{nonsense}")).unwrap();
            let (file, writable) = read(&path);
            assert_eq!(file, ProfileFile::fresh());
            assert!(writable);
            assert!(!path.exists());
        }
        let kept = |suffix: &str| {
            let mut p = path.as_os_str().to_owned();
            p.push(suffix);
            std::fs::read_to_string(PathBuf::from(p)).unwrap()
        };
        assert!(kept(".bad").contains("first"));
        assert!(kept(".bad.2").contains("second"));
    }

    #[test]
    fn a_profile_that_cant_be_read_isnt_saved_over() {
        // A directory where the file should be: it exists, but isn't
        // readable as one.
        let path = temp("unreadable");
        std::fs::create_dir_all(&path).unwrap();
        let mut store = ProfileStore::open(&path);
        assert!(!store.writable);
        let mut file = ProfileFile::fresh();
        file.profile.salvage = 1;
        assert!(!store.save(file));
        assert!(path.is_dir());
    }
}
