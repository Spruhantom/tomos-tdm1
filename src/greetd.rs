//! greetd IPC protocol (length-prefixed JSON over `$GREETD_SOCK`) and the
//! login conversation used by the greeter.
//!
//! The conversation runs on a worker thread. The password typed on the main
//! form is used to answer the *first* secret PAM prompt; any further prompts
//! (one-time codes, expired-password changes, …) are forwarded to the UI.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

pub const MAX_RESPONSE_BYTES: u32 = 1024 * 1024;
const PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    CreateSession { username: String },
    PostAuthMessageResponse { response: Option<String> },
    StartSession { cmd: Vec<String>, env: Vec<String> },
    CancelSession,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success,
    Error { error_type: ErrorType, description: String },
    AuthMessage { auth_message_type: AuthMessageType, auth_message: String },
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    Error,
    AuthError,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMessageType {
    Visible,
    Secret,
    Info,
    Error,
}

pub fn send_request<W: Write>(stream: &mut W, request: &Request) -> Result<(), String> {
    let payload = serde_json::to_vec(request).map_err(|e| format!("Could not encode greetd request: {e}"))?;
    let length = u32::try_from(payload.len()).map_err(|_| "greetd request is too large".to_owned())?;
    stream
        .write_all(&length.to_ne_bytes())
        .and_then(|()| stream.write_all(&payload))
        .and_then(|()| stream.flush())
        .map_err(|e| format!("Could not send greetd request: {e}"))
}

pub fn read_response<R: Read>(stream: &mut R) -> Result<Response, String> {
    let mut length_bytes = [0_u8; 4];
    stream
        .read_exact(&mut length_bytes)
        .map_err(|e| format!("Could not read greetd response: {e}"))?;
    let length = u32::from_ne_bytes(length_bytes);
    if length == 0 || length > MAX_RESPONSE_BYTES {
        return Err(format!("greetd response length is invalid: {length} bytes"));
    }
    let mut payload = vec![0_u8; length as usize];
    stream
        .read_exact(&mut payload)
        .map_err(|e| format!("Could not read greetd response: {e}"))?;
    serde_json::from_slice(&payload).map_err(|e| format!("Could not decode greetd response: {e}"))
}

/// Events sent from the worker thread to the UI.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// PAM needs another answer (not the main password).
    Prompt { message: String, secret: bool },
    /// Informational / warning text from PAM.
    Info(String),
    /// Session accepted by greetd; the greeter should exit.
    Started,
    /// Wrong password or account problem: let the user retry.
    AuthFailed(String),
    /// Anything else that went wrong.
    Failed(String),
    Cancelled,
}

/// Replies sent from the UI to the worker thread.
#[derive(Debug)]
pub enum Reply {
    Answer(String),
    Cancel,
}

/// Everything needed for one login attempt.
#[derive(Debug, Clone)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub command: Vec<String>,
    pub env: Vec<String>,
}

fn cancel(stream: &mut UnixStream) {
    if send_request(stream, &Request::CancelSession).is_ok() {
        let _ = read_response(stream);
    }
}

/// Run a full login conversation against the greetd socket at `socket`.
pub fn login(socket: &str, request: LoginRequest, events: &Sender<Event>, replies: &Receiver<Reply>) {
    let event = match converse(socket, request, events, replies) {
        Ok(event) | Err(event) => event,
    };
    let _ = events.send(event);
}

fn converse(
    socket: &str,
    request: LoginRequest,
    events: &Sender<Event>,
    replies: &Receiver<Reply>,
) -> Result<Event, Event> {
    let fail = |m: String| Event::Failed(m);
    let mut stream = UnixStream::connect(socket).map_err(|e| fail(format!("Cannot connect to greetd: {e}")))?;
    stream.set_read_timeout(Some(PROMPT_TIMEOUT)).map_err(|e| fail(e.to_string()))?;
    stream.set_write_timeout(Some(Duration::from_secs(30))).map_err(|e| fail(e.to_string()))?;

    send_request(&mut stream, &Request::CreateSession { username: request.username.clone() }).map_err(fail)?;
    let mut password = Some(request.password);

    loop {
        match read_response(&mut stream).map_err(fail)? {
            Response::Success => {
                send_request(
                    &mut stream,
                    &Request::StartSession { cmd: request.command.clone(), env: request.env.clone() },
                )
                .map_err(fail)?;
                return match read_response(&mut stream).map_err(fail)? {
                    Response::Success => Ok(Event::Started),
                    Response::Error { description, .. } => {
                        cancel(&mut stream);
                        Err(fail(format!("Could not start the desktop: {description}")))
                    }
                    Response::AuthMessage { .. } => {
                        cancel(&mut stream);
                        Err(fail("Unexpected prompt while starting the desktop.".to_owned()))
                    }
                };
            }
            Response::Error { error_type, description } => {
                cancel(&mut stream);
                return Err(match error_type {
                    ErrorType::AuthError => Event::AuthFailed(friendly_auth_error(&description)),
                    ErrorType::Error => fail(format!("greetd: {description}")),
                });
            }
            Response::AuthMessage { auth_message_type, auth_message } => {
                let answer = match auth_message_type {
                    AuthMessageType::Info | AuthMessageType::Error => {
                        let _ = events.send(Event::Info(auth_message));
                        None
                    }
                    AuthMessageType::Secret if password.is_some() => password.take(),
                    kind => {
                        let secret = kind == AuthMessageType::Secret;
                        events
                            .send(Event::Prompt { message: auth_message, secret })
                            .map_err(|_| fail("Greeter closed.".to_owned()))?;
                        match replies.recv_timeout(PROMPT_TIMEOUT) {
                            Ok(Reply::Answer(answer)) => Some(answer),
                            Ok(Reply::Cancel) => {
                                cancel(&mut stream);
                                return Ok(Event::Cancelled);
                            }
                            Err(RecvTimeoutError::Timeout) => {
                                cancel(&mut stream);
                                return Err(fail("Sign-in timed out. Please try again.".to_owned()));
                            }
                            Err(RecvTimeoutError::Disconnected) => {
                                cancel(&mut stream);
                                return Err(fail("Greeter closed.".to_owned()));
                            }
                        }
                    }
                };
                send_request(&mut stream, &Request::PostAuthMessageResponse { response: answer }).map_err(fail)?;
            }
        }
    }
}

fn friendly_auth_error(description: &str) -> String {
    let d = description.to_ascii_lowercase();
    if d.contains("auth") || d.contains("password") || d.is_empty() {
        "Wrong username or password.".to_owned()
    } else {
        format!("Sign-in refused: {description}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::thread;

    fn frame(json: &serde_json::Value) -> Vec<u8> {
        let payload = serde_json::to_vec(json).unwrap();
        let mut out = (payload.len() as u32).to_ne_bytes().to_vec();
        out.extend(payload);
        out
    }

    fn read_req(stream: &mut UnixStream) -> serde_json::Value {
        let mut len = [0_u8; 4];
        stream.read_exact(&mut len).unwrap();
        let mut payload = vec![0_u8; u32::from_ne_bytes(len) as usize];
        stream.read_exact(&mut payload).unwrap();
        serde_json::from_slice(&payload).unwrap()
    }

    fn write_resp(stream: &mut UnixStream, value: serde_json::Value) {
        stream.write_all(&frame(&value)).unwrap();
    }

    fn socket_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tdm-{tag}-{}-{:?}.sock", std::process::id(), thread::current().id()))
    }

    fn request() -> LoginRequest {
        LoginRequest {
            username: "tom".into(),
            password: "live".into(),
            command: vec!["/usr/bin/tomos-session".into()],
            env: vec!["XDG_SESSION_TYPE=wayland".into()],
        }
    }

    #[test]
    fn decodes_protocol_messages() {
        let mut c = Cursor::new(frame(&serde_json::json!({
            "type": "auth_message", "auth_message_type": "secret", "auth_message": "Password:"
        })));
        assert_eq!(
            read_response(&mut c).unwrap(),
            Response::AuthMessage { auth_message_type: AuthMessageType::Secret, auth_message: "Password:".into() }
        );
    }

    #[test]
    fn encodes_start_session() {
        let mut c = Cursor::new(Vec::new());
        send_request(&mut c, &Request::StartSession { cmd: vec!["a".into()], env: vec!["B=c".into()] }).unwrap();
        let bytes = c.into_inner();
        let v: serde_json::Value = serde_json::from_slice(&bytes[4..]).unwrap();
        assert_eq!(v, serde_json::json!({"type": "start_session", "cmd": ["a"], "env": ["B=c"]}));
    }

    #[test]
    fn rejects_oversized_and_empty_frames() {
        assert!(read_response(&mut Cursor::new((MAX_RESPONSE_BYTES + 1).to_ne_bytes().to_vec())).is_err());
        assert!(read_response(&mut Cursor::new(0_u32.to_ne_bytes().to_vec())).is_err());
    }

    #[test]
    fn password_answers_first_secret_prompt_and_starts_session() {
        let path = socket_path("ok");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            assert_eq!(read_req(&mut s), serde_json::json!({"type": "create_session", "username": "tom"}));
            write_resp(&mut s, serde_json::json!({"type": "auth_message", "auth_message_type": "info", "auth_message": "hello"}));
            assert_eq!(read_req(&mut s)["response"], serde_json::Value::Null);
            write_resp(&mut s, serde_json::json!({"type": "auth_message", "auth_message_type": "secret", "auth_message": "Password:"}));
            assert_eq!(read_req(&mut s)["response"], "live");
            write_resp(&mut s, serde_json::json!({"type": "success"}));
            let start = read_req(&mut s);
            assert_eq!(start["cmd"], serde_json::json!(["/usr/bin/tomos-session"]));
            write_resp(&mut s, serde_json::json!({"type": "success"}));
        });
        let (etx, erx) = mpsc::channel();
        let (_rtx, rrx) = mpsc::channel();
        login(path.to_str().unwrap(), request(), &etx, &rrx);
        server.join().unwrap();
        assert_eq!(erx.recv().unwrap(), Event::Info("hello".into()));
        assert_eq!(erx.recv().unwrap(), Event::Started);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn second_secret_prompt_goes_to_ui() {
        let path = socket_path("otp");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            read_req(&mut s);
            write_resp(&mut s, serde_json::json!({"type": "auth_message", "auth_message_type": "secret", "auth_message": "Password:"}));
            read_req(&mut s);
            write_resp(&mut s, serde_json::json!({"type": "auth_message", "auth_message_type": "secret", "auth_message": "OTP:"}));
            assert_eq!(read_req(&mut s)["response"], "123456");
            write_resp(&mut s, serde_json::json!({"type": "success"}));
            read_req(&mut s);
            write_resp(&mut s, serde_json::json!({"type": "success"}));
        });
        let (etx, erx) = mpsc::channel();
        let (rtx, rrx) = mpsc::channel();
        let p = path.clone();
        let worker = thread::spawn(move || login(p.to_str().unwrap(), request(), &etx, &rrx));
        assert_eq!(erx.recv().unwrap(), Event::Prompt { message: "OTP:".into(), secret: true });
        rtx.send(Reply::Answer("123456".into())).unwrap();
        assert_eq!(erx.recv().unwrap(), Event::Started);
        worker.join().unwrap();
        server.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn wrong_password_reports_auth_failure_and_cancels() {
        let path = socket_path("bad");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            read_req(&mut s);
            write_resp(&mut s, serde_json::json!({"type": "auth_message", "auth_message_type": "secret", "auth_message": "Password:"}));
            read_req(&mut s);
            write_resp(&mut s, serde_json::json!({"type": "error", "error_type": "auth_error", "description": "pam_authenticate: AUTH_ERR"}));
            assert_eq!(read_req(&mut s)["type"], "cancel_session");
            write_resp(&mut s, serde_json::json!({"type": "success"}));
        });
        let (etx, erx) = mpsc::channel();
        let (_rtx, rrx) = mpsc::channel();
        login(path.to_str().unwrap(), request(), &etx, &rrx);
        server.join().unwrap();
        assert_eq!(erx.recv().unwrap(), Event::AuthFailed("Wrong username or password.".into()));
        let _ = std::fs::remove_file(path);
    }
}
