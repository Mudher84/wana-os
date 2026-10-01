use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use wana_power::{Command, SOCKET_PATH};

fn usage() -> ! {
    eprintln!("usage: wana-power {{status|poweroff|reboot}}");
    std::process::exit(2);
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "status".into());
    let command = Command::parse(&arg).unwrap_or_else(|_| usage());
    let mut stream = UnixStream::connect(SOCKET_PATH).unwrap_or_else(|e| {
        eprintln!("wana-power: connect {SOCKET_PATH}: {e}");
        std::process::exit(1);
    });
    writeln!(stream, "{}", command.as_str()).unwrap_or_else(|e| {
        eprintln!("wana-power: write request: {e}");
        std::process::exit(1);
    });
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).unwrap_or_else(|e| {
        eprintln!("wana-power: read reply: {e}");
        std::process::exit(1);
    });
    print!("{reply}");
    if !reply.starts_with("OK ") {
        std::process::exit(1);
    }
}
