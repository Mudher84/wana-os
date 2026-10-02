use std::os::unix::net::UnixDatagram;

fn main() {
    let socket = match UnixDatagram::unbound() {
        Ok(socket) => socket,
        Err(e) => {
            eprintln!("wana-lock: socket: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = socket.send_to(b"lock", wana_auth::LOCK_SOCKET_PATH) {
        eprintln!(
            "wana-lock: cannot request desktop lock through {}: {e}",
            wana_auth::LOCK_SOCKET_PATH
        );
        std::process::exit(1);
    }
    println!("WANA_DESKTOP_LOCK_REQUESTED");
}
