//! Remembers the last successful user and session between boots.
//!
//! Stored in `/var/lib/tdm/state` (owned by the greeter user, see
//! `data/tdm.tmpfiles`). Only a username and a session name are stored,
//! never any credentials.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_PATH: &str = "/var/lib/tdm/state";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub last_user: Option<String>,
    pub last_session: Option<String>,
}

fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 256 && !value.contains('\n')).then(|| value.to_owned())
}

impl State {
    pub fn parse(contents: &str) -> Self {
        let mut state = Self::default();
        for line in contents.lines() {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "user" => state.last_user = clean(value),
                    "session" => state.last_session = clean(value),
                    _ => {}
                }
            }
        }
        state
    }

    pub fn serialize(&self) -> String {
        let mut out = String::new();
        if let Some(user) = &self.last_user {
            out.push_str(&format!("user={user}\n"));
        }
        if let Some(session) = &self.last_session {
            out.push_str(&format!("session={session}\n"));
        }
        out
    }

    pub fn path() -> PathBuf {
        std::env::var_os("TDM_STATE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_PATH))
    }

    pub fn load_from(path: &Path) -> Self {
        fs::read_to_string(path).map(|c| Self::parse(&c)).unwrap_or_default()
    }

    /// Atomically write the state file (write temp file, then rename).
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(self.serialize().as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let state = State { last_user: Some("tom".into()), last_session: Some("Tom OS (Hyprland)".into()) };
        assert_eq!(State::parse(&state.serialize()), state);
    }

    #[test]
    fn saves_atomically() {
        let dir = std::env::temp_dir().join(format!("tdm-state-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state");
        let state = State { last_user: Some("tom".into()), last_session: None };
        state.save_to(&path).unwrap();
        assert_eq!(State::load_from(&path), state);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_is_empty_state() {
        assert_eq!(State::load_from(Path::new("/nonexistent/tdm/state")), State::default());
    }
}
