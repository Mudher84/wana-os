use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
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
const BG: u32 = 0x111827;
const CARD: u32 = 0x1E293B;
const TEXT: u32 = 0xF1F5F9;
const DIM: u32 = 0xA8B3C7;
const ACCENT: u32 = 0x4F8CFF;
const DANGER: u32 = 0xB94A55;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Disk {
    name: String,
    path: PathBuf,
    bytes: u64,
}

fn disks(sys: &Path, source: &Path) -> Result<Vec<Disk>, String> {
    let source_name = source.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let mut out = Vec::new();
    let root = sys.join("class/block");
    for entry in fs::read_dir(&root).map_err(|e| format!("{}: {e}", root.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().join("partition").exists()
            || name == source_name
            || name.starts_with("loop")
            || name.starts_with("ram")
            || name.starts_with("zram")
            || name.starts_with("sr")
            || name.starts_with("dm-")
        {
            continue;
        }
        let sectors = fs::read_to_string(entry.path().join("size"))
            .unwrap_or_default()
            .trim()
            .parse::<u64>()
            .unwrap_or(0);
        if sectors == 0 {
            continue;
        }
        out.push(Disk {
            path: PathBuf::from("/dev").join(&name),
            name,
            bytes: sectors.saturating_mul(512),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Select,
    Confirm,
    Done,
    Failed,
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
    value: &str,
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
    let l = layout(value, fonts, &style)?;
    draw(c, &l, fonts, size, 40.0, y, rgb);
    Ok(())
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
}

fn render(
    fonts: &FontSet,
    source: &Path,
    list: &[Disk],
    selected: usize,
    stage: Stage,
) -> Result<Canvas, String> {
    let mut c = Canvas::new(WIDTH, HEIGHT, BG);
    label(&mut c, fonts, "تثبيت وانا", 24.0, 30.0, TEXT)?;
    label(
        &mut c,
        fonts,
        &format!("المصدر: {}", source.display()),
        68.0,
        15.0,
        DIM,
    )?;
    match stage {
        Stage::Select => {
            label(
                &mut c,
                fonts,
                "اختر القرص الهدف — الأسهم ثم Enter",
                106.0,
                18.0,
                TEXT,
            )?;
            if list.is_empty() {
                label(
                    &mut c,
                    fonts,
                    "لا يوجد قرص صالح للتثبيت",
                    170.0,
                    20.0,
                    DANGER,
                )?;
            }
            for (i, d) in list.iter().take(6).enumerate() {
                let y = 146 + i as u32 * 54;
                fill(
                    &mut c,
                    32,
                    y,
                    WIDTH - 64,
                    46,
                    if i == selected { ACCENT } else { CARD },
                );
                label(
                    &mut c,
                    fonts,
                    &format!("{} — {}", d.path.display(), gib(d.bytes)),
                    y as f32 + 11.0,
                    18.0,
                    TEXT,
                )?;
            }
        }
        Stage::Confirm => {
            fill(&mut c, 32, 150, WIDTH - 64, 180, DANGER);
            label(
                &mut c,
                fonts,
                "تحذير: سيتم مسح القرص بالكامل",
                172.0,
                25.0,
                TEXT,
            )?;
            if let Some(d) = list.get(selected) {
                label(
                    &mut c,
                    fonts,
                    &format!("الهدف: {} — {}", d.path.display(), gib(d.bytes)),
                    222.0,
                    18.0,
                    TEXT,
                )?;
            }
            label(
                &mut c,
                fonts,
                "اضغط Enter مرة ثانية للتثبيت، أو Esc للرجوع",
                274.0,
                17.0,
                TEXT,
            )?;
        }
        Stage::Done => {
            label(
                &mut c,
                fonts,
                "اكتمل التثبيت والتحقق من القراءة بنجاح",
                180.0,
                24.0,
                ACCENT,
            )?;
            label(
                &mut c,
                fonts,
                "يمكنك الآن إعادة التشغيل من القرص المثبت",
                230.0,
                18.0,
                TEXT,
            )?;
        }
        Stage::Failed => {
            label(
                &mut c,
                fonts,
                "فشل التثبيت — لم يتم اعتماد القرص",
                190.0,
                24.0,
                DANGER,
            )?;
        }
    }
    Ok(c)
}

fn invoke_core(source: &Path, target: &Path) -> Result<(), String> {
    let confirm = format!("ERASE:{}", target.display());
    let status = Command::new("/usr/sbin/wana-install")
        .args(["--image"])
        .arg(source)
        .args(["--target"])
        .arg(target)
        .args(["--confirm", &confirm])
        .status()
        .map_err(|e| format!("wana-install: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wana-install exited {status}"))
    }
}

#[derive(Debug)]
struct Args {
    source: PathBuf,
    fonts: PathBuf,
    target: Option<PathBuf>,
    exit_after_install: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        source: PathBuf::from("/run/wana-live/live/Wana-OS.img"),
        fonts: PathBuf::from("/usr/share/fonts/wana"),
        target: None,
        exit_after_install: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--source" => a.source = it.next().ok_or("--source needs a path")?.into(),
            "--fonts" => a.fonts = it.next().ok_or("--fonts needs a path")?.into(),
            "--target" => a.target = Some(it.next().ok_or("--target needs a path")?.into()),
            "--exit-after-install" => a.exit_after_install = true,
            _ => return Err(format!("unknown argument {x}")),
        }
    }
    Ok(a)
}

fn run() -> Result<(), String> {
    let a = parse_args()?;
    let mut list = disks(Path::new("/sys"), &a.source)?;
    if let Some(target) = &a.target {
        list.retain(|d| &d.path == target);
        if list.is_empty() {
            return Err(format!(
                "requested target {} is not an eligible disk",
                target.display()
            ));
        }
    }

    fonts::verify_dir(&a.fonts)?;
    let set = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|n| Font::load(&a.fonts.join(n)))
            .collect::<Result<_, _>>()?,
    };
    let app = App::connect()?;
    let keyboard = app
        .keyboard()?
        .ok_or("installer requires a keyboard seat")?;
    let window = Window::new(
        &app,
        "مثبت وانا",
        "org.wana.Installer",
        WIDTH as i32,
        HEIGHT as i32,
    )?;

    let mut selected = 0usize;
    let mut stage = Stage::Select;
    let mut canvas = render(&set, &a.source, &list, selected, stage)?;
    window.present(&app, &canvas.bytes())?;
    info!(LOG, "installer ready: {} eligible disk(s)", list.len());

    loop {
        app.conn.dispatch()?;
        while let Some(ev) = app.conn.next_event() {
            if window.close_event(&ev) {
                window.destroy(&app);
                return Ok(());
            }
            if let Some((key, pressed)) = app.key_event(keyboard, &ev) {
                if !pressed {
                    continue;
                }
                match (stage, key) {
                    (Stage::Select, 103) if !list.is_empty() => {
                        selected = selected.saturating_sub(1)
                    }
                    (Stage::Select, 108) if !list.is_empty() => {
                        selected = (selected + 1).min(list.len() - 1)
                    }
                    (Stage::Select, 28) if !list.is_empty() => {
                        stage = Stage::Confirm;
                        info!(
                            LOG,
                            "installer confirmation requested for {}",
                            list[selected].path.display()
                        );
                    }
                    (Stage::Confirm, 1) => stage = Stage::Select,
                    (Stage::Confirm, 28) => {
                        let target = list[selected].path.clone();
                        info!(LOG, "installer confirmed for {}", target.display());
                        stage = match invoke_core(&a.source, &target) {
                            Ok(()) => {
                                info!(
                                    LOG,
                                    "installer GUI: install PASS target={}",
                                    target.display()
                                );
                                Stage::Done
                            }
                            Err(e) => {
                                error!(LOG, "installer GUI: {e}");
                                Stage::Failed
                            }
                        };
                        if stage == Stage::Done && a.exit_after_install {
                            window.destroy(&app);
                            return Ok(());
                        }
                    }
                    (_, 1) => {
                        window.destroy(&app);
                        return Ok(());
                    }
                    _ => {}
                }
                canvas = render(&set, &a.source, &list, selected, stage)?;
                let hash = sha256::hex(&sha256::digest(&canvas.bytes()));
                window.present(&app, &canvas.bytes())?;
                info!(
                    LOG,
                    "installer frame: stage={stage:?} selected={selected} sha256 {hash}"
                );
            }
            app.protocol_event(&ev)?;
        }
    }
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "installer GUI: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);
    fn root() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "wana-installer-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(p.join("class/block/vda")).unwrap();
        fs::create_dir_all(p.join("class/block/vdb")).unwrap();
        fs::create_dir_all(p.join("class/block/loop0")).unwrap();
        fs::write(p.join("class/block/vda/size"), "1000\n").unwrap();
        fs::write(p.join("class/block/vdb/size"), "2000\n").unwrap();
        fs::write(p.join("class/block/loop0/size"), "3000\n").unwrap();
        p
    }

    #[test]
    fn disk_discovery_excludes_source_and_virtual_noise() {
        let r = root();
        let got = disks(&r, Path::new("/dev/vda")).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "vdb");
        assert_eq!(got[0].bytes, 2000 * 512);
        let _ = fs::remove_dir_all(r);
    }
}
