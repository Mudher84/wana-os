use wana_power::{request, Command};

fn usage() -> ! {
    eprintln!("usage: wana-power {{status|poweroff|reboot}}");
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(raw) = args.next() else { usage() };
    if args.next().is_some() {
        usage();
    }
    let command = Command::parse(&raw).unwrap_or_else(|_| usage());
    match request(command) {
        Ok(reply) => println!("{reply}"),
        Err(e) => {
            eprintln!("wana-power: {e}");
            std::process::exit(1);
        }
    }
}
