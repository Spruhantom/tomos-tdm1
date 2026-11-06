//! Greeter configuration (`/etc/tdm/greeter.conf`).
//!
//! Simple `key = value` lines; `#` and `;` start comments. Unknown keys are
//! ignored so older greeters keep working with newer configuration files.

use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_PATH: &str = "/etc/tdm/greeter.conf";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Neon,
    Dark,
    Light,
    HighContrast,
}

impl Theme {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "dark" => Self::Dark,
            "light" => Self::Light,
            "high-contrast" | "high_contrast" | "contrast" => Self::HighContrast,
            _ => Self::Neon,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Neon => "neon",
            Self::Dark => "dark",
            Self::Light => "light",
            Self::HighContrast => "high-contrast",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub theme: Theme,
    pub background: Option<PathBuf>,
    pub logo: Option<PathBuf>,
    pub title: String,
    /// Text in the top-left corner (after the bolt icon).
    pub brand: String,
    /// Extra GTK CSS loaded on top of the theme.
    pub css: Option<PathBuf>,
    pub subtitle: String,
    pub default_user: Option<String>,
    pub default_session: Option<String>,
    pub remember: bool,
    pub show_users: bool,
    pub show_power: bool,
    pub clock_format: String,
    pub date_format: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::Neon,
            background: None,
            logo: None,
            title: "TOM OS".to_owned(),
            brand: "TOM OS".to_owned(),
            css: None,
            subtitle: "Welcome to Tom OS".to_owned(),
            default_user: None,
            default_session: None,
            remember: true,
            show_users: true,
            show_power: true,
            clock_format: "%H:%M".to_owned(),
            date_format: "%A %e %B".to_owned(),
        }
    }
}

fn parse_bool(value: &str, fallback: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => fallback,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('"');
    (!value.is_empty()).then(|| value.to_owned())
}

impl Config {
    pub fn parse(contents: &str) -> Self {
        let mut config = Self::default();
        for raw in contents.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') || line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim().to_ascii_lowercase().as_str() {
                "theme" => config.theme = Theme::parse(value),
                "background" => config.background = non_empty(value).map(PathBuf::from),
                "logo" => config.logo = non_empty(value).map(PathBuf::from),
                "title" => config.title = non_empty(value).unwrap_or(config.title),
                "brand" => config.brand = non_empty(value).unwrap_or_default(),
                "css" => config.css = non_empty(value).map(PathBuf::from),
                "subtitle" => config.subtitle = non_empty(value).unwrap_or_default(),
                "default_user" => config.default_user = non_empty(value),
                "default_session" => config.default_session = non_empty(value),
                "remember" => config.remember = parse_bool(value, config.remember),
                "show_users" => config.show_users = parse_bool(value, config.show_users),
                "show_power" => config.show_power = parse_bool(value, config.show_power),
                "clock_format" => config.clock_format = non_empty(value).unwrap_or(config.clock_format),
                "date_format" => config.date_format = non_empty(value).unwrap_or(config.date_format),
                _ => {}
            }
        }
        config
    }

    pub fn load_from(path: &Path) -> Self {
        fs::read_to_string(path).map(|c| Self::parse(&c)).unwrap_or_default()
    }

    /// Load `/etc/tdm/greeter.conf` (or `$TDM_CONFIG`), then apply `$TDM_THEME`.
    pub fn load() -> Self {
        let path = std::env::var_os("TDM_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_PATH));
        let mut config = Self::load_from(&path);
        if let Ok(theme) = std::env::var("TDM_THEME") {
            config.theme = Theme::parse(&theme);
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_neon_theme() {
        assert_eq!(Config::parse("").theme, Theme::Neon);
        assert_eq!(Theme::parse("bogus"), Theme::Neon);
    }

    #[test]
    fn parses_brand_and_css() {
        let c = Config::parse("brand = TOM'S PC\ncss = /etc/tdm/style.css\n");
        assert_eq!(c.brand, "TOM'S PC");
        assert_eq!(c.css.as_deref(), Some(Path::new("/etc/tdm/style.css")));
        let d = Config::parse("");
        assert_eq!(d.brand, "TOM OS");
        assert!(d.css.is_none());
        assert_eq!(Config::parse("brand =\n").brand, "");
    }

    #[test]
    fn parses_all_keys() {
        let c = Config::parse(
            "# comment\ntheme = high-contrast\nbackground=/a.png\nlogo = \"/b.png\"\n\
             default_user=tom\ndefault_session = Tom OS (Hyprland)\nremember=no\n\
             show_power=false\ntitle=HELLO\n",
        );
        assert_eq!(c.theme, Theme::HighContrast);
        assert_eq!(c.background.as_deref(), Some(Path::new("/a.png")));
        assert_eq!(c.logo.as_deref(), Some(Path::new("/b.png")));
        assert_eq!(c.default_user.as_deref(), Some("tom"));
        assert_eq!(c.default_session.as_deref(), Some("Tom OS (Hyprland)"));
        assert!(!c.remember);
        assert!(!c.show_power);
        assert!(c.show_users);
        assert_eq!(c.title, "HELLO");
    }

    #[test]
    fn ignores_garbage_lines() {
        let c = Config::parse("this is not a key\n[section]\n=empty\ntheme=light");
        assert_eq!(c.theme, Theme::Light);
    }
}
