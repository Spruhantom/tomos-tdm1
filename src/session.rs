use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    pub exec: String,
    pub command: Vec<String>,
    pub kind: SessionKind,
    pub file: PathBuf,
    /// Desktop-entry file stem, exported as XDG_SESSION_DESKTOP.
    pub desktop_id: String,
    /// `DesktopNames=` values, exported as XDG_CURRENT_DESKTOP.
    pub desktop_names: Vec<String>,
}

impl Session {
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.kind.label())
    }

    /// Command greetd should run. X11 sessions are started through `startx`
    /// because greetd itself does not start an X server.
    pub fn launch_command(&self) -> Vec<String> {
        match self.kind {
            SessionKind::Wayland => self.command.clone(),
            SessionKind::X11 => {
                let mut cmd = vec!["startx".to_owned(), "/usr/bin/env".to_owned()];
                cmd.extend(self.command.iter().cloned());
                cmd
            }
        }
    }

    /// Environment passed with greetd `start_session`.
    pub fn environment(&self) -> Vec<String> {
        let mut env = vec![
            format!("XDG_SESSION_TYPE={}", self.kind.env_value()),
            format!("XDG_SESSION_DESKTOP={}", self.desktop_id),
        ];
        if !self.desktop_names.is_empty() {
            env.push(format!("XDG_CURRENT_DESKTOP={}", self.desktop_names.join(":")));
        }
        env
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Wayland,
    X11,
}

impl SessionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wayland => "Wayland",
            Self::X11 => "X11",
        }
    }

    pub fn env_value(self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::X11 => "x11",
        }
    }
}

pub fn parse_desktop_entry(path: &Path, kind: SessionKind) -> Result<Option<Session>, String> {
    let contents =
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    parse_desktop_contents(&contents, path, kind)
}

pub fn parse_desktop_contents(
    contents: &str,
    path: &Path,
    kind: SessionKind,
) -> Result<Option<Session>, String> {
    let mut in_desktop_entry = false;
    let mut name = None;
    let mut exec = None;
    let mut try_exec = None;
    let mut hidden = false;
    let mut no_display = false;
    let mut desktop_names = Vec::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "Name" => name = Some(value.trim().to_owned()),
                "Exec" => exec = Some(value.trim().to_owned()),
                "TryExec" => try_exec = Some(value.trim().to_owned()),
                "Hidden" => hidden = value.trim().eq_ignore_ascii_case("true"),
                "NoDisplay" => no_display = value.trim().eq_ignore_ascii_case("true"),
                "DesktopNames" => {
                    desktop_names = value
                        .split(';')
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(str::to_owned)
                        .collect()
                }
                _ => {}
            }
        }
    }

    if hidden || no_display {
        return Ok(None);
    }

    let name = name.unwrap_or_default();
    let exec = exec.unwrap_or_default();
    if name.is_empty() || exec.is_empty() {
        return Err(format!(
            "{}: missing Name or Exec in [Desktop Entry]",
            path.display()
        ));
    }

    if let Some(binary) = try_exec {
        if !binary.is_empty() && !executable_exists(&binary) {
            return Ok(None);
        }
    }

    // Desktop session Exec values are tokenized into argv. They are never
    // passed to a shell, so metacharacters cannot become shell commands.
    let mut command = shlex::split(&exec)
        .ok_or_else(|| format!("{}: invalid quoting in Exec", path.display()))?;
    if command.is_empty() || command[0].is_empty() {
        return Err(format!("{}: Exec contains no executable", path.display()));
    }

    for argument in &mut command {
        let mut expanded = String::with_capacity(argument.len());
        let mut characters = argument.chars();
        while let Some(character) = characters.next() {
            if character == '%' {
                match characters.next() {
                    Some('%') => expanded.push('%'),
                    _ => {
                        return Err(format!(
                            "{}: unsupported desktop-entry field code in Exec",
                            path.display()
                        ));
                    }
                }
            } else {
                expanded.push(character);
            }
        }
        *argument = expanded;
    }

    Ok(Some(Session {
        name,
        exec,
        command,
        kind,
        file: path.to_path_buf(),
        desktop_id: path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_owned(),
        desktop_names,
    }))
}

fn executable_exists(program: &str) -> bool {
    let path = Path::new(program);
    if path.components().count() > 1 {
        return is_executable_file(path);
    }

    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .any(|directory| is_executable_file(&directory.join(program)))
        })
        .unwrap_or(false)
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn scan_directory(path: &Path, kind: SessionKind) -> (Vec<Session>, Vec<String>) {
    let mut sessions = Vec::new();
    let mut warnings = Vec::new();

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (sessions, warnings),
        Err(error) => {
            warnings.push(format!("{}: {error}", path.display()));
            return (sessions, warnings);
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let file = entry.path();
        if file.extension().and_then(|value| value.to_str()) != Some("desktop") {
            continue;
        }
        match parse_desktop_entry(&file, kind) {
            Ok(Some(session)) if !executable_exists(&session.command[0]) => {
                warnings.push(format!(
                    "{}: session executable '{}' was not found or is not executable",
                    file.display(),
                    session.command[0]
                ));
            }
            Ok(Some(session)) => sessions.push(session),
            Ok(None) => {}
            Err(warning) => warnings.push(warning),
        }
    }

    sessions.sort_by_key(|session| session.name.to_lowercase());
    (sessions, warnings)
}

pub fn discover_sessions() -> (Vec<Session>, Vec<String>) {
    let (mut sessions, mut warnings) = scan_directory(
        Path::new("/usr/share/wayland-sessions"),
        SessionKind::Wayland,
    );
    let (x11_sessions, x11_warnings) =
        scan_directory(Path::new("/usr/share/xsessions"), SessionKind::X11);
    if !x11_sessions.is_empty() && !executable_exists("startx") {
        warnings.push("X11 sessions skipped: 'startx' (xinit) is not installed".to_owned());
    } else {
        sessions.extend(x11_sessions);
    }
    warnings.extend(x11_warnings);
    // Tom OS sessions first, then alphabetical.
    sessions.sort_by_key(|s| (!s.desktop_names.iter().any(|n| n == "TomOS"), s.name.to_lowercase()));
    (sessions, warnings)
}

/// Pick the preferred session: remembered name, then configured name, then
/// the first (Tom OS sessions sort first).
pub fn preferred_index(sessions: &[Session], remembered: Option<&str>, configured: Option<&str>) -> usize {
    [remembered, configured]
        .into_iter()
        .flatten()
        .find_map(|wanted| {
            sessions
                .iter()
                .position(|s| s.name == wanted || s.desktop_id == wanted)
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_wayland_desktop_entry_into_safe_argv() {
        let path = Path::new("/tmp/example.desktop");
        let input = "[Desktop Entry]\nName=Example Desktop\nExec=example-session --wayland\nType=Application\n";
        let parsed = parse_desktop_contents(input, path, SessionKind::Wayland).unwrap();
        let session = parsed.unwrap();
        assert_eq!(session.name, "Example Desktop");
        assert_eq!(session.command, ["example-session", "--wayland"]);
        assert_eq!(session.kind, SessionKind::Wayland);
    }

    #[test]
    fn handles_quoted_arguments_without_shell_expansion() {
        let path = Path::new("/tmp/quoted.desktop");
        let input = "[Desktop Entry]\nName=Quoted\nExec=desktop-session --title 'TomOS Desktop'\n";
        let session = parse_desktop_contents(input, path, SessionKind::X11)
            .unwrap()
            .unwrap();
        assert_eq!(
            session.command,
            ["desktop-session", "--title", "TomOS Desktop"]
        );
    }

    #[test]
    fn ignores_hidden_entries() {
        let path = Path::new("/tmp/hidden.desktop");
        let input = "[Desktop Entry]\nName=Hidden\nExec=hidden-session\nHidden=true\n";
        assert!(parse_desktop_contents(input, path, SessionKind::X11)
            .unwrap()
            .is_none());
    }

    #[test]
    fn ignores_entries_outside_desktop_entry_group() {
        let path = Path::new("/tmp/other-group.desktop");
        let input = "[Other]\nName=Wrong Group\nExec=wrong\n";
        assert!(parse_desktop_contents(input, path, SessionKind::X11).is_err());
    }

    #[test]
    fn rejects_missing_exec() {
        let path = Path::new("/tmp/broken.desktop");
        let input = "[Desktop Entry]\nName=Broken\n";
        assert!(parse_desktop_contents(input, path, SessionKind::X11).is_err());
    }

    #[test]
    fn rejects_unclosed_quotes() {
        let path = Path::new("/tmp/unclosed.desktop");
        let input = "[Desktop Entry]\nName=Broken\nExec=desktop-session 'oops\n";
        assert!(parse_desktop_contents(input, path, SessionKind::X11).is_err());
    }

    #[test]
    fn exports_desktop_environment() {
        let path = Path::new("/usr/share/wayland-sessions/tomos.desktop");
        let input = "[Desktop Entry]\nName=Tom OS (Hyprland)\nExec=/usr/bin/tomos-session\nDesktopNames=Hyprland;TomOS\n";
        let s = parse_desktop_contents(input, path, SessionKind::Wayland).unwrap().unwrap();
        assert_eq!(s.desktop_id, "tomos");
        assert_eq!(
            s.environment(),
            [
                "XDG_SESSION_TYPE=wayland",
                "XDG_SESSION_DESKTOP=tomos",
                "XDG_CURRENT_DESKTOP=Hyprland:TomOS"
            ]
        );
        assert_eq!(s.launch_command(), ["/usr/bin/tomos-session"]);
    }

    #[test]
    fn wraps_x11_sessions_in_startx() {
        let path = Path::new("/usr/share/xsessions/i3.desktop");
        let input = "[Desktop Entry]\nName=i3\nExec=i3\n";
        let s = parse_desktop_contents(input, path, SessionKind::X11).unwrap().unwrap();
        assert_eq!(s.launch_command(), ["startx", "/usr/bin/env", "i3"]);
    }

    #[test]
    fn prefers_remembered_then_configured_session() {
        let mk = |name: &str, id: &str| Session {
            name: name.into(),
            exec: "x".into(),
            command: vec!["x".into()],
            kind: SessionKind::Wayland,
            file: PathBuf::from("/x"),
            desktop_id: id.into(),
            desktop_names: vec![],
        };
        let list = vec![mk("Tom OS", "tomos"), mk("Hyprland", "hyprland")];
        assert_eq!(preferred_index(&list, Some("Hyprland"), Some("tomos")), 1);
        assert_eq!(preferred_index(&list, Some("gone"), Some("hyprland")), 1);
        assert_eq!(preferred_index(&list, None, None), 0);
    }

    #[test]
    fn rejects_unsupported_field_codes() {
        let path = Path::new("/tmp/field-code.desktop");
        let input = "[Desktop Entry]\nName=Broken\nExec=desktop-session %f\n";
        assert!(parse_desktop_contents(input, path, SessionKind::X11).is_err());
    }
}
