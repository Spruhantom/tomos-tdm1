use tdm::config::Config;
use tdm::session::discover_sessions;
use tdm::users::discover_users;
use std::env;
use std::process::ExitCode;

fn print_help() {
    println!("TomOS Display Manager (tdm) {}", tdm::VERSION);
    println!();
    println!("USAGE:");
    println!("    tdm <COMMAND>");
    println!();
    println!("COMMANDS:");
    println!("    list-sessions    List installed Wayland and X11 desktop sessions");
    println!("    list-users       List accounts shown in the greeter user picker");
    println!("    config           Show the effective greeter configuration");
    println!("    check            Check sessions, greeter config and greetd setup");
    println!("    version          Print version information");
    println!("    help             Show this help");
}

fn run() -> Result<(), String> {
    let command = env::args().nth(1).unwrap_or_else(|| "help".to_owned());
    match command.as_str() {
        "help" | "--help" | "-h" => print_help(),
        "version" | "--version" | "-V" => println!("tdm {}", tdm::VERSION),
        "list-sessions" | "list" => {
            let (sessions, warnings) = discover_sessions();
            if sessions.is_empty() {
                println!("No desktop sessions found.");
                println!("Looked in /usr/share/wayland-sessions and /usr/share/xsessions.");
            } else {
                println!(
                    "{:<10}  {:<24}  {:<32}  DESKTOP FILE",
                    "TYPE", "SESSION", "COMMAND"
                );
                for session in &sessions {
                    println!(
                        "{:<10}  {:<24}  {:<32}  {}",
                        session.kind.label(),
                        session.name,
                        session.exec,
                        session.file.display()
                    );
                }
                println!();
                println!("{} session(s) found.", sessions.len());
            }
            for warning in warnings {
                eprintln!("warning: {warning}");
            }
        }
        "list-users" | "users" => {
            for user in discover_users() {
                println!("{:<6} {:<16} {}", user.uid, user.name, user.display_name);
            }
        }
        "config" => println!("{:#?}", Config::load()),
        "check" => {
            let (sessions, warnings) = discover_sessions();
            println!("Wayland session directory: /usr/share/wayland-sessions");
            println!("X11 session directory:     /usr/share/xsessions");
            println!("Valid visible sessions:    {}", sessions.len());
            let greetd = std::fs::read_to_string("/etc/greetd/config.toml").unwrap_or_default();
            println!(
                "greetd uses tdm-greeter:   {}",
                if greetd.contains("tdm-greeter") { "yes" } else { "NO (see /etc/greetd/config.toml)" }
            );
            let dm = std::fs::read_link("/etc/systemd/system/display-manager.service")
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "not set".to_owned());
            println!("display-manager.service:   {dm}");
            let config = Config::load();
            println!("Greeter theme:             {}", config.theme.name());
            match &config.css {
                Some(path) if path.is_file() => println!("Custom CSS:                {}", path.display()),
                Some(path) => println!("Custom CSS:                MISSING ({})", path.display()),
                None => println!("Custom CSS:                none"),
            }
            if warnings.is_empty() {
                println!("Session metadata check:    no errors found");
            } else {
                println!("Session metadata check:    {} warning(s)", warnings.len());
                for warning in warnings {
                    eprintln!("warning: {warning}");
                }
            }
        }
        other => return Err(format!("unknown command '{other}'. Run 'tdm help'.")),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tdm: {error}");
            ExitCode::FAILURE
        }
    }
}
