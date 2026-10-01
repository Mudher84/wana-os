use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};
use wana_client::app::{App, Window};
use wana_log::{error, info, Subsystem};
use wana_text::bidi::Base;
use wana_text::font::Font;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::raster::{draw, Canvas};
use wana_text::{fonts, sha256};

const LOG: Subsystem = Subsystem::Shell;
const WIDTH: u32 = 760;
const HEIGHT: u32 = 520;
const BG: u32 = wana_theme::color::BG_DARK;
const CARD: u32 = wana_theme::color::CARD_DARK;
const TEXT: u32 = wana_theme::color::TEXT_DARK;
const DIM: u32 = wana_theme::color::DIM_DARK;
const ACCENT: u32 = wana_theme::color::ACCENT_BLUE;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    dir: bool,
    symlink: bool,
}

fn scan(path: &Path) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    for item in fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))? {
        let item = item.map_err(|e| format!("{}: {e}", path.display()))?;
        let ty = item
            .file_type()
            .map_err(|e| format!("{}: {e}", item.path().display()))?;
        out.push(Entry {
            name: item.file_name().to_string_lossy().into_owned(),
            dir: ty.is_dir(),
            symlink: ty.is_symlink(),
        });
    }
    out.sort_by(|a, b| {
        b.dir.cmp(&a.dir).then_with(|| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        })
    });
    Ok(out)
}

fn fill(c: &mut Canvas, x: u32, y: u32, w: u32, h: u32, rgb: u32) {
    for row in y..(y + h).min(c.height) {
        let start = (row * c.width + x.min(c.width)) as usize;
        let end = (row * c.width + (x + w).min(c.width)) as usize;
        c.pixels[start..end].fill(0xFF00_0000 | rgb);
    }
}

fn label(
    c: &mut Canvas,
    fonts: &FontSet,
    s: &str,
    y: f32,
    size: f32,
    rgb: u32,
) -> Result<(), String> {
    let style = Style {
        size,
        base: Base::Rtl,
        align: Align::Start,
        width: Some(c.width as f32 - 80.0),
        language: "ar".into(),
    };
    let l = layout(s, fonts, &style)?;
    draw(c, &l, fonts, size, 40.0, y, rgb);
    Ok(())
}

fn render(fonts: &FontSet, path: &Path, entries: &[Entry]) -> Result<Canvas, String> {
    let mut c = Canvas::new(WIDTH, HEIGHT, BG);
    label(&mut c, fonts, "الملفات", 24.0, 30.0, TEXT)?;
    label(
        &mut c,
        fonts,
        &format!("المسار: {}", path.display()),
        68.0,
        16.0,
        DIM,
    )?;

    if entries.is_empty() {
        label(&mut c, fonts, "المجلد فارغ", 140.0, 20.0, DIM)?;
        return Ok(c);
    }

    for (i, e) in entries.iter().take(8).enumerate() {
        let y = 112 + i as u32 * 48;
        fill(&mut c, 32, y, WIDTH - 64, 40, CARD);
        let kind = if e.symlink {
            "رابط"
        } else if e.dir {
            "مجلد"
        } else {
            "ملف"
        };
        label(
            &mut c,
            fonts,
            &format!("{kind} — {}", e.name),
            y as f32 + 9.0,
            17.0,
            if e.dir { ACCENT } else { TEXT },
        )?;
    }
    if entries.len() > 8 {
        label(
            &mut c,
            fonts,
            &format!("و {} عناصر أخرى", entries.len() - 8),
            500.0,
            15.0,
            DIM,
        )?;
    }
    Ok(c)
}

#[derive(Debug)]
struct Args {
    path: PathBuf,
    fonts: PathBuf,
    hold: Option<u64>,
    list: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut out = Args {
        path: PathBuf::from("/"),
        fonts: PathBuf::from("/usr/share/fonts/wana"),
        hold: None,
        list: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--path" => out.path = it.next().ok_or("--path needs a directory")?.into(),
            "--fonts" => out.fonts = it.next().ok_or("--fonts needs a directory")?.into(),
            "--hold" => {
                out.hold = Some(
                    it.next()
                        .ok_or("--hold needs seconds")?
                        .parse()
                        .map_err(|e| format!("--hold: {e}"))?,
                );
            }
            "--list" => out.list = true,
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    Ok(out)
}

fn run() -> Result<(), String> {
    let a = parse_args()?;
    let entries = scan(&a.path)?;
    if a.list {
        for e in &entries {
            println!(
                "{}\t{}",
                if e.symlink {
                    "link"
                } else if e.dir {
                    "dir"
                } else {
                    "file"
                },
                e.name
            );
        }
        return Ok(());
    }

    fonts::verify_dir(&a.fonts)?;
    let set = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|n| Font::load(&a.fonts.join(n)))
            .collect::<Result<_, _>>()?,
    };
    let canvas = render(&set, &a.path, &entries)?;
    let hash = sha256::hex(&sha256::digest(&canvas.bytes()));
    let app = App::connect()?;
    let window = Window::new(
        &app,
        "الملفات — وانا",
        "org.wana.Files",
        WIDTH as i32,
        HEIGHT as i32,
    )?;
    window.present(&app, &canvas.bytes())?;
    info!(
        LOG,
        "files mapped: {}x{}, path={}, entries={}, sha256 {hash}",
        WIDTH,
        HEIGHT,
        a.path.display(),
        entries.len()
    );

    let deadline = a.hold.map(|s| Instant::now() + Duration::from_secs(s));
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        app.conn.wait(250)?;
        while let Some(ev) = app.conn.next_event() {
            if window.close_event(&ev) {
                window.destroy(&app);
                return Ok(());
            }
            app.protocol_event(&ev)?;
        }
    }
    window.destroy(&app);
    Ok(())
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "files: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);

    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "wana-files-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn scan_sorts_directories_first_and_does_not_follow_links() {
        let root = temp();
        fs::create_dir_all(root.join("folder")).unwrap();
        fs::write(root.join("b.txt"), b"b").unwrap();
        fs::write(root.join("A.txt"), b"a").unwrap();
        symlink(root.join("folder"), root.join("link")).unwrap();
        let got = scan(&root).unwrap();
        assert_eq!(got[0].name, "folder");
        assert!(got[0].dir);
        let link = got.iter().find(|e| e.name == "link").unwrap();
        assert!(link.symlink);
        assert!(!link.dir);
        let _ = fs::remove_dir_all(root);
    }
}
