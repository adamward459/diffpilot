//! How `diffpilot-mcp` finds the app. A run passes `--socket` and `--token`. A terminal agent starts it with
//! `--terminal`, which uses the app's own socket and the terminal token.

use super::bridge::socket_path_for;
use crate::settings::app_support_dir;
use std::path::PathBuf;

pub const TERMINAL_TOKEN: &str = "terminal";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub socket: PathBuf,
    pub token: String,
}

/// `args` are the command-line arguments after the program name. The environment values are the fallback.
pub fn parse(
    args: impl IntoIterator<Item = String>,
    env_socket: Option<String>,
    env_token: Option<String>,
) -> Result<Launch, &'static str> {
    let mut socket = env_socket.map(PathBuf::from);
    let mut token = env_token;
    let mut terminal = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => socket = args.next().map(PathBuf::from),
            "--token" => token = args.next(),
            "--terminal" => terminal = true,
            _ => {}
        }
    }
    if terminal {
        let default = app_support_dir().map(|dir| socket_path_for(&dir));
        return match socket.or(default) {
            Some(socket) => Ok(Launch {
                socket,
                token: TERMINAL_TOKEN.into(),
            }),
            None => Err("diffpilot-mcp cannot find the app's data folder. Pass --socket."),
        };
    }
    match (socket, token) {
        (Some(socket), Some(token)) => Ok(Launch { socket, token }),
        _ => Err("diffpilot-mcp needs --socket and --token, or --terminal."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn a_run_passes_the_socket_and_token() {
        assert_eq!(
            parse(args(&["--socket", "/s", "--token", "t"]), None, None),
            Ok(Launch {
                socket: "/s".into(),
                token: "t".into()
            })
        );
        assert_eq!(
            parse(args(&[]), Some("/env".into()), Some("e".into())),
            Ok(Launch {
                socket: "/env".into(),
                token: "e".into()
            })
        );
        assert!(parse(args(&["--socket", "/s"]), None, None).is_err());
    }

    #[test]
    fn terminal_mode_uses_the_terminal_token_and_the_default_socket() {
        let launch = parse(args(&["--terminal"]), None, None).unwrap();
        assert_eq!(launch.token, TERMINAL_TOKEN);
        assert_eq!(launch.socket, socket_path_for(&app_support_dir().unwrap()));
        let custom = parse(args(&["--terminal", "--socket", "/custom.sock"]), None, None).unwrap();
        assert_eq!(custom.socket, PathBuf::from("/custom.sock"));
    }
}
