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
const KEY_ESC: u32 = 1;
const KEY_BACKSPACE: u32 = 14;
const KEY_ENTER: u32 = 28;
const KEY_UP: u32 = 103;
const KEY_DOWN: u32 = 108;
const PAGE_ROWS: usize = 8;

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

fn visible_start(selected: usize, len: usize) -> usize {
    if len <= PAGE_ROWS {
        0
    } else {
        selected
            .saturating_sub(PAGE_ROWS - 1)
            .min(len.saturating_sub(PAGE_ROWS))
    }
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

fn render(
    fonts: &FontSet,
    path: &Path,
    entries: &[Entry],
    selected: usize,
) -> Result<Canvas, String> {
    let palette = wana_theme::current();
    let mut c = Canvas::new(WIDTH, HEIGHT, palette.bg);
    label(&mut c, fonts, "الملفات", 20.0, 28.0, palette.text)?;
    label(
        &mut c,
        fonts,
        &format!("المسار: {}", path.display()),
        60.0,
        15.0,
        palette.dim,
    )?;

    if entries.is_empty() {
        label(&mut c, fonts, "المجلد فارغ", 140.0, 20.0, palette.dim)?;
    } else {
        let start = visible_start(selected, entries.len());
        for (row, (index, entry)) in entries
            .iter()
            .enumerate()
            .skip(start)
            .take(PAGE_ROWS)
            .enumerate()
        {
            let y = 96 + row as u32 * 46;
            let active = index == selected;
            fill(
                &mut c,
                32,
                y,
                WIDTH - 64,
                38,
                if active { palette.accent } else { palette.card },
            );
            let kind = if entry.symlink {
                "رابط"
            } else if entry.dir {
                "مجلد"
            } else {
                "ملف"
            };
            let color = if active {
                wana_theme::color::TEXT_DARK
            } else if entry.dir {
                palette.accent
            } else {
                palette.text
            };
            label(
                &mut c,
                fonts,
                &format!("{kind} — {}", entry.name),
                y as f32 + 8.0,
                16.0,
                color,
            )?;
        }
        let end = (start + PAGE_ROWS).min(entries.len());
        label(
            &mut c,
            fonts,
            &format!("{}–{} من {}", start + 1, end, entries.len()),
            466.0,
            13.0,
            palette.dim,
        )?;
    }

    label(
        &mut c,
        fonts,
        "↑↓ اختيار   Enter فتح مجلد   Backspace رجوع   Esc خروج",
        492.0,
        12.0,
        palette.dim,
    )?;
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

fn present(
    app: &App,
    window: &Window,
    fonts: &FontSet,
    path: &Path,
    entries: &[Entry],
    selected: usize,
) -> Result<String, String> {
    let canvas = render(fonts, path, entries, selected)?;
    let hash = sha256::hex(&sha256::digest(&canvas.bytes()));
    window.present(app, &canvas.bytes())?;
    Ok(hash)
}

fn run() -> Result<(), String> {
    let a = parse_args()?;
    let mut path = fs::canonicalize(&a.path)
        .map_err(|e| format!("{}: {e}", a.path.display()))?;
    let mut entries = scan(&path)?;
    if a.list {
        for entry in &entries {
            println!(
                "{}\t{}",
                if entry.symlink {
                    "link"
                } else if entry.dir {
                    "dir"
                } else {
                    "file"
                },
                entry.name
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

    let app = App::connect()?;
    let keyboard = app.keyboard()?.ok_or("files requires a keyboard seat")?;
    let window = Window::new(
        &app,
        "الملفات — وانا",
        "org.wana.Files",
        WIDTH as i32,
        HEIGHT as i32,
    )?;
    let mut selected = 0usize;
    let hash = present(&app, &window, &set, &path, &entries, selected)?;
    info!(
        LOG,
        "files mapped: {}x{}, path={}, entries={}, selected={}, sha256 {hash}",
        WIDTH,
        HEIGHT,
        path.display(),
        entries.len(),
        selected
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
            if let Some((key, pressed)) = app.key_event(keyboard, &ev) {
                if !pressed {
                    continue;
                }
                let mut redraw = false;
                match key {
                    KEY_UP if !entries.is_empty() => {
                        selected = selected.saturating_sub(1);
                        redraw = true;
                    }
                    KEY_DOWN if !entries.is_empty() => {
                        selected = (selected + 1).min(entries.len() - 1);
                        redraw = true;
                    }
                    KEY_ENTER if !entries.is_empty() && entries[selected].dir => {
                        let next = path.join(&entries[selected].name);
                        path = fs::canonicalize(&next)
                            .map_err(|e| format!("{}: {e}", next.display()))?;
                        entries = scan(&path)?;
                        selected = 0;
                        redraw = true;
                        info!(LOG, "files entered directory: {}", path.display());
                    }
                    KEY_BACKSPACE => {
                        if let Some(parent) = path.parent() {
                            if parent != path {
                                path = parent.to_path_buf();
                                entries = scan(&path)?;
                                selected = 0;
                                redraw = true;
                                info!(LOG, "files moved to parent: {}", path.display());
                            }
                        }
                    }
                    KEY_ESC => {
                        window.destroy(&app);
                        return Ok(());
                    }
                    _ => {}
                }
                if redraw {
                    let hash = present(&app, &window, &set, &path, &entries, selected)?;
                    info!(
                        LOG,
                        "files view: path={} entries={} selected={} sha256 {hash}",
                        path.display(),
                        entries.len(),
                        selected
                    );
                }
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

    #[test]
    fn visible_window_follows_selection() {
        assert_eq!(visible_start(0, 20), 0);
        assert_eq!(visible_start(7, 20), 0);
        assert_eq!(visible_start(8, 20), 1);
        assert_eq!(visible_start(19, 20), 12);
        assert_eq!(visible_start(4, 5), 0);
    }
}
