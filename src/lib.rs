//! TomOS Display Manager (TDM)
//!
//! A small greetd greeter written in Rust. The non-GUI parts live in this
//! library so they can be unit-tested without a display:
//!
//! * [`config`]  – `/etc/tdm/greeter.conf` parsing
//! * [`session`] – Wayland / X11 desktop-entry discovery
//! * [`users`]   – human account discovery from `/etc/passwd`
//! * [`state`]   – remembered last user and session
//! * [`greetd`]  – the greetd JSON IPC protocol and login conversation

pub mod config;
pub mod greetd;
pub mod session;
pub mod state;
pub mod users;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
