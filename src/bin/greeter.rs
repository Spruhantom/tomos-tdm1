//! tdm-greeter — the TomOS Display Manager login screen (GTK4, run by greetd
//! inside the `cage` kiosk compositor).

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
#[cfg(test)]
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use tdm::config::{Config, Theme};
use tdm::greetd::{self, Event, LoginRequest, Reply};
use tdm::session::{self, Session};
use tdm::state::State;
use tdm::users::{self, User};

// ───────────────────────────── Theme ─────────────────────────────

struct Palette {
    bg: &'static str,
    card: &'static str,
    fg: &'static str,
    muted: &'static str,
    accent: &'static str,  // Tom OS pink
    accent2: &'static str, // cyan
    accent3: &'static str, // purple
    warn: &'static str,
    error: &'static str,
    card_alpha: &'static str,
    glow: bool,
}

const NEON: Palette = Palette {
    bg: "#0a0a12",
    card: "#14141f",
    fg: "#e8e8f0",
    muted: "#8a8aa0",
    accent: "#ff2d96",
    accent2: "#00f0ff",
    accent3: "#c74cff",
    warn: "#ffe14d",
    error: "#ff2d78",
    card_alpha: "0.80",
    glow: true,
};
const DARK: Palette = Palette {
    bg: "#101318",
    card: "#191f28",
    fg: "#e8f5ff",
    muted: "#8da3b8",
    accent: "#37d9ff",
    accent2: "#37d9ff",
    accent3: "#2785a4",
    warn: "#ffc66d",
    error: "#ff6b6b",
    card_alpha: "0.92",
    glow: false,
};
const LIGHT: Palette = Palette {
    bg: "#f5f7fa",
    card: "#ffffff",
    fg: "#17212b",
    muted: "#40566a",
    accent: "#c2006a",
    accent2: "#006b88",
    accent3: "#6a2a9a",
    warn: "#714500",
    error: "#b00030",
    card_alpha: "0.94",
    glow: false,
};
const HIGH_CONTRAST: Palette = Palette {
    bg: "#000000",
    card: "#000000",
    fg: "#ffffff",
    muted: "#ffffff",
    accent: "#ffff00",
    accent2: "#00ffff",
    accent3: "#ffffff",
    warn: "#ffff00",
    error: "#ff8080",
    card_alpha: "1.0",
    glow: false,
};

const CSS_TEMPLATE: &str = r#"
window.tdm { background-color: @BG@; color: @FG@;
  font-family: "JetBrainsMono Nerd Font", "JetBrains Mono", "TDM Icons", "DejaVu Sans Mono", monospace; }
.tdm-scrim { background-image: linear-gradient(180deg, alpha(@BG@,0.70), alpha(@BG@,0.10) 30%, alpha(@BG@,0.10) 70%, alpha(@BG@,0.80)); }
.tdm-bar { padding: 16px 28px; }
.tdm-brand { color: @ACCENT@; font-weight: 900; letter-spacing: 4px; font-size: 15px; text-shadow: @GLOW_ACCENT@; }
.tdm-host { color: @MUTED@; font-size: 13px; }
.tdm-clock { color: @ACCENT2@; font-size: 26px; font-weight: 900; text-shadow: @GLOW_ACCENT2@; }
.tdm-date { color: @FG@; font-size: 13px; }
.tdm-card { background-color: alpha(@CARD@,@CARD_ALPHA@); border: 2px solid alpha(@ACCENT@,0.80);
  border-radius: 20px; padding: 30px 38px 26px 38px; box-shadow: @CARD_SHADOW@; }
.tdm-title { color: @ACCENT@; font-size: 36px; font-weight: 900; letter-spacing: 8px; text-shadow: @GLOW_ACCENT@; }
.tdm-subtitle { color: @ACCENT2@; font-size: 14px; letter-spacing: 3px; text-shadow: @GLOW_ACCENT2@; }
.tdm-label { color: @ACCENT3@; font-size: 11px; font-weight: 900; letter-spacing: 3px; }
.tdm-card entry, .tdm-card dropdown > button {
  background-image: none; background-color: alpha(@BG@,0.90); color: @FG@;
  border: 1px solid alpha(@ACCENT@,0.50); border-radius: 10px; padding: 6px 12px; min-height: 30px;
  box-shadow: none; caret-color: @ACCENT2@; }
.tdm-card entry:focus-within, .tdm-card dropdown > button:focus, .tdm-card dropdown > button:hover {
  border-color: @ACCENT2@; box-shadow: @FOCUS_SHADOW@; }
.tdm-card entry > text > placeholder { color: alpha(@MUTED@,0.85); }
popover > contents, popover > arrow { background-color: @CARD@; border: 1px solid @ACCENT@; color: @FG@; }
popover listview > row:selected, popover row:hover { background-color: alpha(@ACCENT@,0.30); }
button.tdm-signin { background-image: linear-gradient(90deg, @ACCENT@, @ACCENT3@ 55%, @ACCENT2@);
  color: @BG@; font-weight: 900; letter-spacing: 4px; font-size: 14px; border: none; border-radius: 10px;
  padding: 10px; min-height: 26px; box-shadow: @SIGNIN_SHADOW@; text-shadow: none; }
button.tdm-signin:hover { box-shadow: @SIGNIN_HOVER@; }
button.tdm-signin:disabled { background-image: none; background-color: alpha(@MUTED@,0.35); color: @FG@; box-shadow: none; }
button.tdm-ghost { background-image: none; background-color: transparent; color: @MUTED@; border: 1px solid alpha(@MUTED@,0.5);
  border-radius: 10px; padding: 8px 14px; box-shadow: none; }
button.tdm-ghost:hover { color: @FG@; border-color: @FG@; }
button.tdm-power { background-image: none; background-color: alpha(@CARD@,0.75); color: @FG@;
  border: 1px solid alpha(@ACCENT3@,0.60); border-radius: 999px; padding: 6px 18px; font-weight: 700; box-shadow: none; }
button.tdm-power:hover { color: @WARN@; border-color: @WARN@; box-shadow: @POWER_HOVER@; }
.tdm-status { color: @FG@; font-size: 12px; }
.tdm-status.error { color: @ERROR@; font-weight: 800; }
.tdm-status.busy { color: @ACCENT2@; }
.tdm-caps { color: @WARN@; font-size: 12px; font-weight: 900; }
.tdm-footer { color: @MUTED@; font-size: 11px; letter-spacing: 2px; }
*:focus-visible { outline: 2px solid @ACCENT2@; outline-offset: 2px; }
"#;

fn palette(theme: Theme) -> &'static Palette {
    match theme {
        Theme::Neon => &NEON,
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
        Theme::HighContrast => &HIGH_CONTRAST,
    }
}

fn build_css(theme: Theme) -> String {
    let p = palette(theme);
    let glow = |c: &str, r: u8| {
        if p.glow { format!("0 0 {r}px alpha({c},0.85)") } else { "none".to_owned() }
    };
    let (card_shadow, focus, signin, signin_hover, power_hover) = if p.glow {
        (
            format!("0 0 26px alpha({a},0.45), 0 0 70px alpha({b},0.18), inset 0 0 0 1px alpha({b},0.25)", a = p.accent, b = p.accent2),
            format!("0 0 10px alpha({},0.60)", p.accent2),
            format!("0 0 16px alpha({},0.60)", p.accent),
            format!("0 0 26px alpha({},0.85)", p.accent2),
            format!("0 0 12px alpha({},0.60)", p.warn),
        )
    } else {
        let n = || "none".to_owned();
        (n(), format!("0 0 0 2px {}", p.accent2), n(), n(), n())
    };
    CSS_TEMPLATE
        .replace("@GLOW_ACCENT@", &glow(p.accent, 12))
        .replace("@GLOW_ACCENT2@", &glow(p.accent2, 10))
        .replace("@CARD_SHADOW@", &card_shadow)
        .replace("@FOCUS_SHADOW@", &focus)
        .replace("@SIGNIN_SHADOW@", &signin)
        .replace("@SIGNIN_HOVER@", &signin_hover)
        .replace("@POWER_HOVER@", &power_hover)
        .replace("@CARD_ALPHA@", p.card_alpha)
        .replace("@BG@", p.bg)
        .replace("@CARD@", p.card)
        .replace("@FG@", p.fg)
        .replace("@MUTED@", p.muted)
        .replace("@ACCENT@", p.accent)
        .replace("@ACCENT2@", p.accent2)
        .replace("@ACCENT3@", p.accent3)
        .replace("@WARN@", p.warn)
        .replace("@ERROR@", p.error)
}

// ───────────────────────────── Helpers ─────────────────────────────

const DEFAULT_BACKGROUNDS: &[&str] = &[
    "/usr/share/backgrounds/tomos/wallpaper-eclipse.png",
    "/usr/share/backgrounds/tomos/wallpaper-eclipse-1080.png",
    "/usr/share/backgrounds/tomos/wallpaper.png",
];
const DEFAULT_LOGOS: &[&str] = &[
    "/usr/share/tdm/logo.png",
    "/usr/share/icons/hicolor/256x256/apps/tomos.png",
];

fn first_existing(configured: &Option<PathBuf>, defaults: &[&str]) -> Option<PathBuf> {
    configured
        .iter()
        .cloned()
        .chain(defaults.iter().map(PathBuf::from))
        .find(|p| p.is_file())
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| std::fs::read_to_string("/etc/hostname"))
        .map(|h| h.trim().to_owned())
        .unwrap_or_else(|_| "tomos".to_owned())
}

fn now(format: &str) -> String {
    glib::DateTime::now_local()
        .ok()
        .and_then(|d| d.format(format).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l
}

fn field_label(text: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Label {
    let l = gtk::Label::with_mnemonic(text);
    l.add_css_class("tdm-label");
    l.set_halign(gtk::Align::Start);
    l.set_mnemonic_widget(Some(widget));
    l
}

fn set_status(status: &gtk::Label, text: &str, kind: &str) {
    for class in ["error", "busy"] {
        status.remove_css_class(class);
    }
    if !kind.is_empty() {
        status.add_css_class(kind);
    }
    status.set_text(text);
}

// ───────────────────────────── UI ─────────────────────────────

struct Ui {
    window: gtk::ApplicationWindow,
    user_dropdown: gtk::DropDown,
    username: gtk::Entry,
    password: gtk::PasswordEntry,
    extra_label: gtk::Label,
    extra: gtk::PasswordEntry,
    extra_box: gtk::Box,
    session_dropdown: gtk::DropDown,
    signin: gtk::Button,
    cancel: gtk::Button,
    status: gtk::Label,
    users: Vec<User>,
    sessions: Vec<Session>,
    config: Config,
    busy: Cell<bool>,
    awaiting_extra: Cell<bool>,
    reply_tx: RefCell<Option<Sender<Reply>>>,
    event_tx: Sender<Event>,
    pending: RefCell<Option<(String, String)>>, // (user, session name) for the state file
}

impl Ui {
    fn selected_username(&self) -> String {
        let idx = self.user_dropdown.selected() as usize;
        match self.users.get(idx) {
            Some(user) if self.config.show_users => user.name.clone(),
            _ => self.username.text().trim().to_owned(),
        }
    }

    fn other_user_selected(&self) -> bool {
        !self.config.show_users || self.users.is_empty() || self.user_dropdown.selected() as usize >= self.users.len()
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.user_dropdown.set_sensitive(!busy);
        self.username.set_sensitive(!busy);
        self.password.set_sensitive(!busy);
        self.session_dropdown.set_sensitive(!busy && !self.sessions.is_empty());
        self.cancel.set_visible(busy);
        self.signin.set_sensitive(!busy && !self.sessions.is_empty());
        self.signin.set_label(if busy { "SIGNING IN…" } else { "SIGN IN" });
        if !busy {
            self.awaiting_extra.set(false);
            self.extra_box.set_visible(false);
            *self.reply_tx.borrow_mut() = None;
        }
    }

    fn submit(self: &Rc<Self>) {
        if self.awaiting_extra.get() {
            let answer = self.extra.text().to_string();
            self.extra.set_text("");
            if let Some(tx) = self.reply_tx.borrow().as_ref() {
                let _ = tx.send(Reply::Answer(answer));
            }
            self.awaiting_extra.set(false);
            self.signin.set_sensitive(false);
            set_status(&self.status, "Checking…", "busy");
            return;
        }
        if self.busy.get() {
            return;
        }
        let username = self.selected_username();
        if !users::is_valid_username(&username) {
            set_status(&self.status, "Enter a valid username.", "error");
            self.username.grab_focus();
            return;
        }
        let Some(session) = self.sessions.get(self.session_dropdown.selected() as usize).cloned() else {
            set_status(&self.status, "No desktop session is installed.", "error");
            return;
        };
        let socket = match std::env::var("GREETD_SOCK") {
            Ok(s) => s,
            Err(_) => {
                set_status(&self.status, "GREETD_SOCK is not set — TDM must be started by greetd.", "error");
                return;
            }
        };
        let request = LoginRequest {
            username: username.clone(),
            password: self.password.text().to_string(),
            command: session.launch_command(),
            env: session.environment(),
        };
        self.password.set_text("");
        *self.pending.borrow_mut() = Some((username, session.name.clone()));
        let (reply_tx, reply_rx) = mpsc::channel();
        *self.reply_tx.borrow_mut() = Some(reply_tx);
        self.set_busy(true);
        set_status(&self.status, "Signing in…", "busy");
        let events = self.event_tx.clone();
        thread::spawn(move || greetd::login(&socket, request, &events, &reply_rx));
    }

    fn cancel_login(&self) {
        if let Some(tx) = self.reply_tx.borrow().as_ref() {
            let _ = tx.send(Reply::Cancel);
            set_status(&self.status, "Cancelling…", "busy");
        }
    }

    fn handle(self: &Rc<Self>, event: Event, app: &gtk::Application) -> bool {
        match event {
            Event::Prompt { message, secret } => {
                self.extra_label.set_text(&message.trim().trim_end_matches(':').to_uppercase());
                self.extra.set_show_peek_icon(secret);
                self.extra_box.set_visible(true);
                self.awaiting_extra.set(true);
                self.signin.set_sensitive(true);
                self.signin.set_label("CONTINUE");
                set_status(&self.status, &message, "busy");
                self.extra.grab_focus();
            }
            Event::Info(message) => set_status(&self.status, &message, "busy"),
            Event::Started => {
                set_status(&self.status, "Welcome to Tom OS — starting your desktop…", "busy");
                if self.config.remember {
                    if let Some((user, session)) = self.pending.borrow_mut().take() {
                        let state = State { last_user: Some(user), last_session: Some(session) };
                        if let Err(e) = state.save_to(&State::path()) {
                            eprintln!("tdm-greeter: could not save state: {e}");
                        }
                    }
                }
                app.quit();
                return false;
            }
            Event::AuthFailed(message) => {
                self.set_busy(false);
                set_status(&self.status, &message, "error");
                self.password.grab_focus();
            }
            Event::Failed(message) => {
                self.set_busy(false);
                set_status(&self.status, &message, "error");
            }
            Event::Cancelled => {
                self.set_busy(false);
                set_status(&self.status, "Sign-in cancelled.", "");
                self.password.grab_focus();
            }
        }
        true
    }
}

fn power_button(icon: &str, text: &str, action: &'static str, question: &'static str, window: &gtk::ApplicationWindow, status: &gtk::Label) -> gtk::Button {
    let button = gtk::Button::with_label(&format!("{icon}  {text}"));
    button.add_css_class("tdm-power");
    button.set_tooltip_text(Some(question));
    let window = window.clone();
    let status = status.clone();
    button.connect_clicked(move |_| {
        let dialog = gtk::AlertDialog::builder()
            .modal(true)
            .message(question)
            .buttons(["Cancel", "Continue"])
            .cancel_button(0)
            .default_button(1)
            .build();
        let status = status.clone();
        dialog.choose(Some(&window), None::<&gio::Cancellable>, move |result| {
            if result == Ok(1) {
                match Command::new("loginctl").arg(action).status() {
                    Ok(s) if s.success() => set_status(&status, "Request accepted…", "busy"),
                    Ok(s) => set_status(&status, &format!("Power request denied ({s})."), "error"),
                    Err(e) => set_status(&status, &format!("Could not run loginctl: {e}"), "error"),
                }
            }
        });
    });
    button
}

fn build(app: &gtk::Application) {
    let config = Config::load();
    let state = if config.remember { State::load_from(&State::path()) } else { State::default() };

    let provider = gtk::CssProvider::new();
    provider.load_from_string(&build_css(config.theme));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_USER);
        gtk::Settings::for_display(&display)
            .set_gtk_application_prefer_dark_theme(config.theme != Theme::Light);
        // Optional user CSS (css = /etc/tdm/style.css), loaded after the theme so it wins.
        if let Some(path) = config.css.as_ref().filter(|p| p.is_file()) {
            let custom = gtk::CssProvider::new();
            custom.connect_parsing_error(|_, section, error| {
                eprintln!("tdm-greeter: custom CSS {}: {}", section.to_str(), error);
            });
            custom.load_from_path(path);
            gtk::style_context_add_provider_for_display(&display, &custom, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);
        } else if let Some(path) = &config.css {
            eprintln!("tdm-greeter: custom CSS not found: {}", path.display());
        }
    }

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Tom OS — Sign in")
        .default_width(1280)
        .default_height(800)
        .build();
    window.add_css_class("tdm");

    // Background wallpaper + scrim
    let overlay = gtk::Overlay::new();
    if let Some(bg) = first_existing(&config.background, DEFAULT_BACKGROUNDS) {
        let picture = gtk::Picture::for_filename(&bg);
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_can_shrink(true);
        overlay.set_child(Some(&picture));
    }
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("tdm-scrim");
    root.set_hexpand(true);
    root.set_vexpand(true);
    overlay.add_overlay(&root);

    // Top bar
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    top.add_css_class("tdm-bar");
    let brand = label(&format!("\u{f0e7}  {}", config.brand), "tdm-brand");
    brand.set_visible(!config.brand.is_empty());
    let host = label(&format!("@ {}", hostname()), "tdm-host");
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let clock_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let clock = label(&now(&config.clock_format), "tdm-clock");
    clock.set_halign(gtk::Align::End);
    let date = label(&now(&config.date_format), "tdm-date");
    date.set_halign(gtk::Align::End);
    clock_box.append(&clock);
    clock_box.append(&date);
    top.append(&brand);
    top.append(&host);
    top.append(&spacer);
    top.append(&clock_box);
    root.append(&top);

    // Centre card
    let centre = gtk::Box::new(gtk::Orientation::Vertical, 0);
    centre.set_vexpand(true);
    centre.set_valign(gtk::Align::Center);
    centre.set_halign(gtk::Align::Center);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.add_css_class("tdm-card");
    card.set_width_request(420);

    if let Some(logo) = first_existing(&config.logo, DEFAULT_LOGOS) {
        let image = gtk::Image::from_file(&logo);
        image.set_pixel_size(112);
        image.set_margin_bottom(4);
        card.append(&image);
    }
    let title = label(&config.title, "tdm-title");
    card.append(&title);
    if !config.subtitle.is_empty() {
        let subtitle = label(&config.subtitle, "tdm-subtitle");
        subtitle.set_margin_bottom(10);
        card.append(&subtitle);
    }

    // User picker
    let users_list = if config.show_users { users::discover_users() } else { Vec::new() };
    let mut user_labels: Vec<String> = users_list.iter().map(|u| {
        if u.display_name == u.name { format!("\u{f007}  {}", u.name) } else { format!("\u{f007}  {} ({})", u.display_name, u.name) }
    }).collect();
    user_labels.push("\u{f234}  Other user…".to_owned());
    let refs: Vec<&str> = user_labels.iter().map(String::as_str).collect();
    let user_dropdown = gtk::DropDown::from_strings(&refs);
    let username = gtk::Entry::builder().placeholder_text("username").hexpand(true).build();
    username.set_input_purpose(gtk::InputPurpose::FreeForm);

    let remembered_user = state.last_user.clone().or_else(|| config.default_user.clone());
    if let Some(i) = remembered_user.as_deref().and_then(|n| users_list.iter().position(|u| u.name == n)) {
        user_dropdown.set_selected(i as u32);
    } else if let Some(name) = &remembered_user {
        user_dropdown.set_selected(users_list.len() as u32);
        username.set_text(name);
    }
    let show_picker = config.show_users && !users_list.is_empty();
    let user_label_target: gtk::Widget = if show_picker { user_dropdown.clone().upcast() } else { username.clone().upcast() };
    card.append(&field_label("_USER", &user_label_target));
    if show_picker {
        card.append(&user_dropdown);
    }
    card.append(&username);

    let password = gtk::PasswordEntry::builder().show_peek_icon(true).hexpand(true).activates_default(false).build();
    password.set_placeholder_text(Some("password"));
    card.append(&field_label("_PASSWORD", &password));
    card.append(&password);

    let caps = label("\u{f071}  CAPS LOCK IS ON", "tdm-caps");
    caps.set_halign(gtk::Align::Start);
    caps.set_visible(false);
    card.append(&caps);

    // Extra PAM prompt (OTP, password change …)
    let extra_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let extra = gtk::PasswordEntry::builder().hexpand(true).build();
    let extra_label = field_label("RESPONSE", &extra);
    extra_box.append(&extra_label);
    extra_box.append(&extra);
    extra_box.set_visible(false);
    card.append(&extra_box);

    // Session picker
    let (sessions, warnings) = session::discover_sessions();
    for w in &warnings {
        eprintln!("tdm-greeter: {w}");
    }
    let session_labels: Vec<String> = sessions.iter().map(|s| format!("\u{f108}  {}", s.name)).collect();
    let srefs: Vec<&str> = session_labels.iter().map(String::as_str).collect();
    let session_dropdown = if srefs.is_empty() {
        gtk::DropDown::from_strings(&["No desktop sessions found"])
    } else {
        gtk::DropDown::from_strings(&srefs)
    };
    session_dropdown.set_selected(session::preferred_index(
        &sessions,
        state.last_session.as_deref(),
        config.default_session.as_deref(),
    ) as u32);
    session_dropdown.set_sensitive(!sessions.is_empty());
    card.append(&field_label("_SESSION", &session_dropdown));
    card.append(&session_dropdown);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    actions.set_margin_top(10);
    let signin = gtk::Button::with_label("SIGN IN");
    signin.add_css_class("tdm-signin");
    signin.set_hexpand(true);
    signin.set_sensitive(!sessions.is_empty());
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("tdm-ghost");
    cancel.set_visible(false);
    actions.append(&signin);
    actions.append(&cancel);
    card.append(&actions);

    let status = label(
        if sessions.is_empty() { "No Wayland or X11 sessions are installed." } else { "" },
        "tdm-status",
    );
    status.set_wrap(true);
    status.set_max_width_chars(44);
    status.set_halign(gtk::Align::Center);
    status.set_justify(gtk::Justification::Center);
    card.append(&status);

    centre.append(&card);
    root.append(&centre);

    // Bottom bar
    let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    bottom.add_css_class("tdm-bar");
    if config.show_power {
        bottom.append(&power_button("\u{f186}", "Suspend", "suspend", "Suspend this computer?", &window, &status));
        bottom.append(&power_button("\u{f0e2}", "Restart", "reboot", "Restart this computer?", &window, &status));
        bottom.append(&power_button("\u{f011}", "Shut down", "poweroff", "Shut down this computer?", &window, &status));
    }
    let spacer2 = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer2.set_hexpand(true);
    bottom.append(&spacer2);
    bottom.append(&label(&format!("TDM {}  ·  TOMOS DISPLAY MANAGER", tdm::VERSION), "tdm-footer"));
    root.append(&bottom);

    window.set_child(Some(&overlay));

    let (event_tx, event_rx): (Sender<Event>, Receiver<Event>) = mpsc::channel();
    let ui = Rc::new(Ui {
        window: window.clone(),
        user_dropdown,
        username,
        password,
        extra_label,
        extra,
        extra_box,
        session_dropdown,
        signin,
        cancel,
        status,
        users: users_list,
        sessions,
        config,
        busy: Cell::new(false),
        awaiting_extra: Cell::new(false),
        reply_tx: RefCell::new(None),
        event_tx,
        pending: RefCell::new(None),
    });

    // Show the typed-username entry only for "Other user…"
    let sync_other = {
        let ui = ui.clone();
        move || {
            let other = ui.other_user_selected();
            ui.username.set_visible(other);
            if other { ui.username.grab_focus(); } else { ui.password.grab_focus(); }
        }
    };
    sync_other();
    {
        let sync_other = sync_other.clone();
        ui.user_dropdown.connect_selected_notify(move |_| sync_other());
    }

    { let ui2 = ui.clone(); ui.username.connect_activate(move |_| { ui2.password.grab_focus(); }); }
    { let ui2 = ui.clone(); ui.password.connect_activate(move |_| ui2.submit()); }
    { let ui2 = ui.clone(); ui.extra.connect_activate(move |_| ui2.submit()); }
    { let ui2 = ui.clone(); ui.signin.connect_clicked(move |_| ui2.submit()); }
    { let ui2 = ui.clone(); ui.cancel.connect_clicked(move |_| ui2.cancel_login()); }

    // Escape cancels a running sign-in
    let keys = gtk::EventControllerKey::new();
    { let ui2 = ui.clone(); keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape && ui2.busy.get() { ui2.cancel_login(); return glib::Propagation::Stop; }
        glib::Propagation::Proceed
    }); }
    window.add_controller(keys);

    // Caps Lock indicator
    if let Some(keyboard) = gdk::Display::default().and_then(|d| d.default_seat()).and_then(|s| s.keyboard()) {
        caps.set_visible(keyboard.is_caps_locked());
        let caps2 = caps.clone();
        keyboard.connect_caps_lock_state_notify(move |k| caps2.set_visible(k.is_caps_locked()));
    }

    // Clock
    {
        let fmt_c = ui.config.clock_format.clone();
        let fmt_d = ui.config.date_format.clone();
        glib::timeout_add_seconds_local(1, move || {
            clock.set_text(&now(&fmt_c));
            date.set_text(&now(&fmt_d));
            glib::ControlFlow::Continue
        });
    }

    // Worker → UI events
    {
        let ui2 = ui.clone();
        let app2 = app.clone();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            while let Ok(event) = event_rx.try_recv() {
                if !ui2.handle(event, &app2) {
                    return glib::ControlFlow::Break;
                }
            }
            glib::ControlFlow::Continue
        });
    }

    { let ui2 = ui.clone(); window.connect_close_request(move |_| { ui2.cancel_login(); glib::Propagation::Proceed }); }

    // Request fullscreen only after the surface is mapped. Asking before the
    // first commit crashes cage 0.2.x / wlroots 0.19
    // (wlr_xdg_surface_schedule_configure: surface->initialized assertion).
    // Under cage the window is already fullscreen, so this is cosmetic there.
    if std::env::var_os("TDM_WINDOWED").is_none() {
        window.connect_map(|w| {
            let w = w.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                w.fullscreen();
            });
        });
    }
    window.present();
    let _ = &ui.window;
}

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("tdm-greeter {}", tdm::VERSION);
        return glib::ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--print-css") {
        println!("{}", build_css(Config::load().theme));
        return glib::ExitCode::SUCCESS;
    }
    let app = gtk::Application::builder()
        .application_id("org.tomos.DisplayManager.Greeter")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(build);
    // GTK must not see our own flags.
    app.run_with_args(&args[..1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_css_has_no_unreplaced_placeholders() {
        for theme in [Theme::Neon, Theme::Dark, Theme::Light, Theme::HighContrast] {
            let css = build_css(theme);
            assert!(!css.contains('@') || !css.contains("@ACCENT"), "placeholder left in {theme:?}");
            assert!(!css.contains("@BG@"));
        }
    }

    #[test]
    fn every_theme_falls_back_to_the_bundled_icon_font() {
        // data/fonts/TDMIcons.otf carries the icons for distros without a Nerd Font
        for theme in [Theme::Neon, Theme::Dark, Theme::Light, Theme::HighContrast] {
            assert!(build_css(theme).contains("\"TDM Icons\""), "{theme:?}");
        }
    }

    #[test]
    fn neon_theme_uses_tomos_palette() {
        let css = build_css(Theme::Neon);
        assert!(css.contains("#ff2d96") && css.contains("#00f0ff") && css.contains("#c74cff"));
    }

    #[test]
    fn picks_first_existing_path() {
        let missing = Some(PathBuf::from("/nonexistent.png"));
        assert_eq!(first_existing(&missing, &["/also/missing"]), None);
        assert_eq!(first_existing(&None, &["/etc/hostname", "/nope"]).is_some(), Path::new("/etc/hostname").is_file());
    }
}
