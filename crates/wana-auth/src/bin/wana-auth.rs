use std::io::{self, Read};
use wana_auth::{status, AuthStatus};

fn read_password() -> Result<String, String> {
    let mut input = String::new();
    io::stdin()
        .take((wana_auth::MAX_PASSWORD_BYTES + 2) as u64)
        .read_to_string(&mut input)
        .map_err(|e| format!("read password: {e}"))?;
    while input.ends_with('\n') || input.ends_with('\r') {
        input.pop();
    }
    Ok(input)
}

fn main() {
    let command = std::env::args().nth(1).unwrap_or_else(|| "status".into());
    let result = match command.as_str() {
        "status" => status().map(|state| match state {
            AuthStatus::SetupRequired => "WANA_AUTH_SETUP_REQUIRED".to_string(),
            AuthStatus::Ready => "WANA_AUTH_READY".to_string(),
        }),
        "setup" => read_password()
            .and_then(|password| wana_auth::setup(&password))
            .map(|()| "WANA_AUTH_SETUP_PASS".to_string()),
        "verify" => read_password()
            .and_then(|password| wana_auth::verify(&password))
            .and_then(|ok| {
                if ok {
                    Ok("WANA_AUTH_VERIFY_PASS".to_string())
                } else {
                    Err("invalid credential".into())
                }
            }),
        _ => Err("usage: wana-auth {status|setup|verify}; setup/verify read password from stdin".into()),
    };
    match result {
        Ok(line) => println!("{line}"),
        Err(e) => {
            eprintln!("wana-auth: {e}");
            std::process::exit(1);
        }
    }
}
