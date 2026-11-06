//! Human account discovery for the user picker.

use std::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub display_name: String,
    pub uid: u32,
}

const MIN_UID: u32 = 1000;
const MAX_UID: u32 = 60000;

fn is_login_shell(shell: &str) -> bool {
    !(shell.is_empty()
        || shell.ends_with("/nologin")
        || shell.ends_with("/false")
        || shell.ends_with("/sync"))
}

/// Parse `/etc/passwd`-formatted text into login-capable human users.
pub fn parse_passwd(contents: &str) -> Vec<User> {
    let mut users: Vec<User> = contents
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() < 7 || fields[0].is_empty() || fields[0].starts_with('#') {
                return None;
            }
            let uid: u32 = fields[2].parse().ok()?;
            if !(MIN_UID..MAX_UID).contains(&uid) || !is_login_shell(fields[6]) {
                return None;
            }
            let gecos = fields[4].split(',').next().unwrap_or("").trim();
            Some(User {
                name: fields[0].to_owned(),
                display_name: if gecos.is_empty() { fields[0].to_owned() } else { gecos.to_owned() },
                uid,
            })
        })
        .collect();
    users.sort_by_key(|u| u.uid);
    users.dedup_by(|a, b| a.name == b.name);
    users
}

pub fn discover_users() -> Vec<User> {
    fs::read_to_string("/etc/passwd").map(|c| parse_passwd(&c)).unwrap_or_default()
}

/// Validate a typed username before sending it to greetd.
pub fn is_valid_username(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '@' | '$'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\n\
        _greetd:x:998:998::/var/lib/greetd:/bin/false\n\
        tom:x:1000:1000:Tom,,,:/home/tom:/bin/bash\n\
        guest:x:1001:1001::/home/guest:/usr/bin/zsh\n\
        svc:x:1002:1002::/srv:/usr/sbin/nologin\n\
        nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n";

    #[test]
    fn keeps_only_human_login_accounts() {
        let users = parse_passwd(PASSWD);
        let names: Vec<_> = users.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["tom", "guest"]);
        assert_eq!(users[0].display_name, "Tom");
        assert_eq!(users[1].display_name, "guest");
    }

    #[test]
    fn validates_usernames() {
        assert!(is_valid_username("tom"));
        assert!(is_valid_username("first.last"));
        assert!(!is_valid_username(""));
        assert!(!is_valid_username("-rf"));
        assert!(!is_valid_username("tom smith"));
        assert!(!is_valid_username("a/b"));
    }
}
