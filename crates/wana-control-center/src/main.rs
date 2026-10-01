mod draw;
mod store;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use store::Store;
use wana_client::app::{App, Window};
use wana_log::{error, info, Subsystem};
use wana_text::font::Font;
use wana_text::layout::FontSet;
use wana_text::{fonts, sha256};

const LOG: Subsystem = Subsystem::Shell;

#[derive(Debug)]
enum Command {
    Ui,
    Notify {
        app: String,
        title: String,
        body: String,
    },
    List,
    Clear,
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
    let mut font_dir = PathBuf::from(fonts::DEFAULT_DIR);
    let mut hold = None;
    let mut test_seed = false;
    let mut positionals = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--root" => root = it.next().ok_or("--root needs a path")?.into(),
            "--fonts" => font_dir = it.next().ok_or("--fonts needs a path")?.into(),
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
        None | Some("ui") if positionals.len() <= 1 => Command::Ui,
        Some("notify") if positionals.len() == 4 => Command::Notify {
            app: positionals[1].clone(),
            title: positionals[2].clone(),
            body: positionals[3].clone(),
        },
        Some("list") if positionals.len() == 1 => Command::List,
        Some("clear") if positionals.len() == 1 => Command::Clear,
        Some(command) => return Err(format!("invalid command or arguments: {command}")),
        None => return Err("invalid arguments".into()),
    };

    Ok(Args {
        root,
        fonts: font_dir,
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
            "notification mutation requires uid 0; current effective uid is {uid}"
        ))
    }
}

fn seed(store: &Store) -> Result<(), String> {
    require_root()?;
    store.clear()?;
    store.push(
        "org.wana.Network",
        "الشبكة متصلة",
        "تم الاتصال بالشبكة المحلية بنجاح",
    )?;
    store.push(
        "org.wana.Files",
        "اكتمل النسخ",
        "تم نسخ الملفات المطلوبة بدون أخطاء",
    )?;
    store.push(
        "org.wana.Security",
        "فحص الأمان",
        "لا توجد أحداث أمنية تحتاج إلى تدخل",
    )?;
    Ok(())
}

fn ui(args: &Args, store: &Store) -> Result<(), String> {
    if args.test_seed {
        seed(store)?;
    }

    fonts::verify_dir(&args.fonts)?;
    let fonts = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|name| Font::load(&args.fonts.join(name)))
            .collect::<Result<_, _>>()?,
    };
    let notifications = store.list()?;
    let canvas = draw::center(&fonts, &notifications)?;
    let hash = sha256::hex(&sha256::digest(&canvas.bytes()));

    let app = App::connect()?;
    let window = Window::new(
        &app,
        "مركز التحكم — وانا",
        "org.wana.ControlCenter",
        draw::WIDTH as i32,
        draw::HEIGHT as i32,
    )?;
    window.present(&app, &canvas.bytes())?;
    info!(
        LOG,
        "control center mapped: {}x{} notifications={} sha256 {hash}",
        draw::WIDTH,
        draw::HEIGHT,
        notifications.len()
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
        Command::Notify { app, title, body } => {
            let seq = store.push(app, title, body)?;
            info!(LOG, "notification stored: seq={seq} app={app}");
            Ok(())
        }
        Command::List => {
            for notification in store.list()? {
                println!(
                    "{}\t{}\t{}\t{}",
                    notification.seq, notification.app, notification.title, notification.body
                );
            }
            Ok(())
        }
        Command::Clear => {
            store.clear()?;
            info!(LOG, "notification history cleared");
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error_message) => {
            error!(LOG, "control-center: {error_message}");
            ExitCode::FAILURE
        }
    }
}
