use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

const SOCKET: &str = "/run/wana/update.sock";

fn main() {
    let command = std::env::args().nth(1).unwrap_or_else(|| "status".into());
    if !matches!(command.as_str(), "status" | "fetch-stage" | "clear") {
        eprintln!("usage: wana-update-client {{status|fetch-stage|clear}}");
        std::process::exit(2);
    }
    let mut stream = UnixStream::connect(SOCKET).unwrap_or_else(|e| {
        eprintln!("wana-update-client: connect {SOCKET}: {e}");
        std::process::exit(1);
    });
    writeln!(stream, "{command}").unwrap_or_else(|e| {
        eprintln!("wana-update-client: write: {e}");
        std::process::exit(1);
    });
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).unwrap_or_else(|e| {
        eprintln!("wana-update-client: read: {e}");
        std::process::exit(1);
    });
    print!("{reply}");
    if reply.starts_with("ERR ") {
        std::process::exit(1);
    }
}
