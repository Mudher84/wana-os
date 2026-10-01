mod draw;
mod store;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use store::{Decision, Store};
use wana_client::app::{App, Window};
use wana_log::{error, info, Subsystem};
use wana_text::font::Font;
use wana_text::layout::FontSet;
use wana_text::{fonts, sha256};

const LOG: Subsystem = Subsystem::Shell;

#[derive(Debug)]
enum Command {
    Ui,
    Set {
        app: String,
        permission: String,
        decision: Decision,
    },
    Check {
        app: String,
        permission: String,
    },
    List {
        app: Option<String>,
    },
    Audit {
        limit: usize,
    },
    ClearAudit,
}

#[derive(Debug)]
struct Args {
    root: PathBuf,
    fonts: PathBuf,
    hold: Option<u64>,
    test_seed: bool,
    command: Command,
}

fn parse_args() -> Result<Args, String> {
    let mut root = Store::system().root().to_path_buf();
    let mut fonts = PathBuf::from(fonts::DEFAULT_DIR);
    let mut hold = None;
    let mut test_seed = false;
    let mut positionals = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--root" => root = it.next().ok_or("--root needs a path")?.into(),
            "--fonts" => fonts = it.next().ok_or("--fonts needs a path")?.into(),
            "--hold" => {
                hold = Some(
                    it.next()
                        .ok_or("--hold needs seconds")?
                        .parse()
                        .map_err(|e| format!("--hold: {e}"))?,
                )
            }
            "--test-seed" => test_seed = true,
            _ => positionals.push(arg),
        }
    }

    let command = match positionals.first().map(String::as_str) {
        None | Some("ui") => {
            if positionals.len() > 1 {
                return Err("ui takes no positional arguments".into());
            }
            Command::Ui
        }
        Some("set") if positionals.len() == 4 => Command::Set {
            app: positionals[1].clone(),
            permission: positionals[2].clone(),
            decision: Decision::parse(&positionals[3])?,
        },
        Some("check") if positionals.len() == 3 => Command::Check {
            app: positionals[1].clone(),
            permission: positionals[2].clone(),
        },
        Some("list") if positionals.len() <= 2 => Command::List {
            app: positionals.get(1).cloned(),
        },
        Some("audit") if positionals.len() <= 2 => Command::Audit {
            limit: positionals
                .get(1)
                .map(|value| value.parse::<usize>())
                .transpose()
                .map_err(|e| format!("audit limit: {e}"))?
                .unwrap_or(32)
                .min(store::MAX_AUDIT),
        },
        Some("clear-audit") if positionals.len() == 1 => Command::ClearAudit,
        Some(command) => return Err(format!("invalid command or arguments: {command}")),
    };

    Ok(Args {
        root,
        fonts,
        hold,
        test_seed,
        command,
    })
}

fn require_root() -> Result<(), String> {
    let uid = store::effective_uid()?;
    if uid == 0 {
        Ok(())
    } else {
        Err(format!(
            "permission mutation requires uid 0; current effective uid is {uid}"
        ))
    }
}

fn seed(store: &Store) -> Result<(), String> {
    require_root()?;
    store.set("settings", "org.wana.Files", "files.read", Decision::Allow)?;
    store.set("settings", "org.wana.Files", "files.write", Decision::Deny)?;
    store.set("settings", "org.wana.Network", "network", Decision::Allow)?;
    store.set(
        "settings",
        "org.wana.Installer",
        "storage.raw",
        Decision::Deny,
    )?;
    Ok(())
}

fn ui(args: &Args, store: &Store) -> Result<(), String> {
    if args.test_seed {
        store.clear_audit()?;
        seed(store)?;
    }

    fonts::verify_dir(&args.fonts)?;
    let fonts = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|name| Font::load(&args.fonts.join(name)))
            .collect::<Result<_, _>>()?,
    };
    let mut rules = store.rules()?;
    let mut audit = store.audit()?;

    let app = App::connect()?;
    let keyboard = app
        .keyboard()?
        .ok_or("permission center requires a keyboard seat")?;
    let window = Window::new(
        &app,
        "الأذونات والخصوصية — وانا",
        "org.wana.Permissions",
        draw::WIDTH as i32,
        draw::HEIGHT as i32,
    )?;
    let mut selected = 0usize;
    let mut canvas = draw::center_selected(
        &fonts,
        &rules,
        &audit,
        (!rules.is_empty()).then_some(selected),
    )?;
    let mut hash = sha256::hex(&sha256::digest(&canvas.bytes()));
    window.present(&app, &canvas.bytes())?;
    info!(
        LOG,
        "permission center mapped: {}x{} rules={} audit={} sha256 {hash}",
        draw::WIDTH,
        draw::HEIGHT,
        rules.len(),
        audit.len()
    );

    let deadline = args
        .hold
        .map(|seconds| Instant::now() + Duration::from_secs(seconds));
    loop {
        if deadline.is_some_and(|value| Instant::now() >= value) {
            break;
        }
        app.conn.wait(250)?;
        while let Some(event) = app.conn.next_event() {
            if window.close_event(&event) {
                window.destroy(&app);
                return Ok(());
            }
            if let Some((key, pressed)) = app.key_event(keyboard, &event) {
                if !pressed {
                    continue;
                }
                let mut redraw = false;
                match key {
                    103 if !rules.is_empty() => {
                        selected = selected.saturating_sub(1);
                        redraw = true;
                    }
                    108 if !rules.is_empty() => {
                        selected = (selected + 1).min(rules.len().min(4) - 1);
                        redraw = true;
                    }
                    105 | 106 | 28 if !rules.is_empty() => {
                        let current = rules[selected].decision;
                        let decision = match key {
                            105 => Decision::Deny,
                            106 => Decision::Allow,
                            _ => {
                                if current == Decision::Allow {
                                    Decision::Deny
                                } else {
                                    Decision::Allow
                                }
                            }
                        };
                        let rule = rules[selected].clone();
                        store.set(
                            "settings-ui",
                            &rule.app,
                            &rule.permission,
                            decision,
                        )?;
                        rules = store.rules()?;
                        audit = store.audit()?;
                        selected = selected.min(rules.len().saturating_sub(1));
                        info!(
                            LOG,
                            "permission UI saved: app={} permission={} decision={}",
                            rule.app,
                            rule.permission,
                            decision.as_str()
                        );
                        redraw = true;
                    }
                    111 => {
                        store.clear_audit()?;
                        audit = store.audit()?;
                        info!(LOG, "permission UI cleared audit history");
                        redraw = true;
                    }
                    1 => {
                        window.destroy(&app);
                        return Ok(());
                    }
                    _ => {}
                }
                if redraw {
                    canvas = draw::center_selected(
                        &fonts,
                        &rules,
                        &audit,
                        (!rules.is_empty()).then_some(selected),
                    )?;
                    hash = sha256::hex(&sha256::digest(&canvas.bytes()));
                    window.present(&app, &canvas.bytes())?;
                    info!(
                        LOG,
                        "permission center updated: rules={} audit={} selected={} sha256 {hash}",
                        rules.len(),
                        audit.len(),
                        selected
                    );
                }
            }
            app.protocol_event(&event)?;
        }
    }
    window.destroy(&app);
    Ok(())
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let store = Store::new(&args.root);
    match &args.command {
        Command::Ui => ui(&args, &store),
        Command::Set {
            app,
            permission,
            decision,
        } => {
            store.set("settings", app, permission, *decision)?;
            info!(
                LOG,
                "permission policy saved: app={app} permission={permission} decision={}",
                decision.as_str()
            );
            Ok(())
        }
        Command::Check { app, permission } => {
            let decision = store.decision(app, permission)?;
            store.record_check("broker", app, permission, decision)?;
            println!("{}", decision.as_str());
            info!(
                LOG,
                "permission check: app={app} permission={permission} decision={}",
                decision.as_str()
            );
            Ok(())
        }
        Command::List { app } => {
            for rule in store.rules()? {
                if app.as_ref().is_none_or(|filter| filter == &rule.app) {
                    println!(
                        "{}\t{}\t{}",
                        rule.app,
                        rule.permission,
                        rule.decision.as_str()
                    );
                }
            }
            Ok(())
        }
        Command::Audit { limit } => {
            let audit = store.audit()?;
            let start = audit.len().saturating_sub(*limit);
            for entry in &audit[start..] {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    entry.seq,
                    entry.action,
                    entry.actor,
                    entry.app,
                    entry.permission,
                    entry.decision.as_str()
                );
            }
            Ok(())
        }
        Command::ClearAudit => {
            store.clear_audit()?;
            info!(LOG, "permission audit cleared");
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error_message) => {
            error!(LOG, "permissions: {error_message}");
            ExitCode::FAILURE
        }
    }
}
