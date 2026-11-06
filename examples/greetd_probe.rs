//! Headless end-to-end check: runs the same login conversation as the
//! greeter against a real greetd. Usage (as a greetd greeter command):
//!   greetd_probe <user> <password> <session-command...>
use std::sync::mpsc;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let socket = std::env::var("GREETD_SOCK").expect("GREETD_SOCK");
    let (etx, erx) = mpsc::channel();
    let (_rtx, rrx) = mpsc::channel();
    let req = tdm::greetd::LoginRequest {
        username: args[0].clone(),
        password: args[1].clone(),
        command: args[2..].to_vec(),
        env: vec!["XDG_SESSION_TYPE=wayland".into(), "XDG_SESSION_DESKTOP=tomos".into()],
    };
    tdm::greetd::login(&socket, req, &etx, &rrx);
    for e in erx.try_iter() { eprintln!("probe event: {e:?}"); }
}
