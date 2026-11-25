use diffpilot_core::mcp::protocol;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

struct Link {
    socket: String,
    token: String,
    stream: RefCell<Option<(BufReader<UnixStream>, UnixStream)>>,
}

impl Link {
    fn exchange(&self, line: &str) -> std::io::Result<String> {
        let mut slot = self.stream.borrow_mut();
        if slot.is_none() {
            let stream = UnixStream::connect(&self.socket)?;
            *slot = Some((BufReader::new(stream.try_clone()?), stream));
        }
        let (reader, writer) = slot.as_mut().expect("connected above");
        writer.write_all(line.as_bytes())?;
        writer.flush()?;
        let mut answer = String::new();
        if reader.read_line(&mut answer)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "diffpilot closed the connection",
            ));
        }
        Ok(answer)
    }

    fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
        let mut line = json!({ "token": self.token, "tool": tool, "args": args }).to_string();
        line.push('\n');
        let mut last = String::new();
        for _ in 0..2 {
            match self.exchange(&line) {
                Ok(answer) => {
                    let parsed: Value = serde_json::from_str(answer.trim())
                        .map_err(|error| format!("bad answer from diffpilot: {error}"))?;
                    if parsed["ok"] == true {
                        return Ok(parsed.get("result").cloned().unwrap_or(Value::Null));
                    }
                    return Err(parsed["error"].as_str().unwrap_or("unknown error").to_string());
                }
                Err(error) => {
                    last = error.to_string();
                    *self.stream.borrow_mut() = None;
                }
            }
        }
        Err(format!("diffpilot is not reachable ({last}). Is it still running?"))
    }
}

/// An MCP server over stdio. `claude` starts it from a run's `--mcp-config`, or from a terminal agent's `claude mcp add`.
fn main() -> ExitCode {
    let launch = diffpilot_core::mcp::launch::parse(
        std::env::args().skip(1),
        std::env::var("DIFFPILOT_MCP_SOCKET").ok(),
        std::env::var("DIFFPILOT_MCP_TOKEN").ok(),
    );
    let launch = match launch {
        Ok(launch) => launch,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let link = Link {
        socket: launch.socket.to_string_lossy().into_owned(),
        token: launch.token,
        stream: RefCell::new(None),
    };
    let forward = |tool: &str, args: Value| link.call(tool, args);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(response) = protocol::handle(&request, &forward)
            && (writeln!(stdout, "{response}").is_err() || stdout.flush().is_err())
        {
            break;
        }
    }
    ExitCode::SUCCESS
}
