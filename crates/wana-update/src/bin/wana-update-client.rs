fn usage() -> ! {
    eprintln!("usage: wana-update-client {{status|fetch-stage|clear}}");
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else { usage() };
    if args.next().is_some() {
        usage();
    }
    match wana_update::broker_request(&command) {
        Ok(reply) => println!("{reply}"),
        Err(e) => {
            eprintln!("wana-update-client: {e}");
            std::process::exit(1);
        }
    }
}
