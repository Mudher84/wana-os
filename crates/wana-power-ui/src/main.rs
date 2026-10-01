use std::path::PathBuf;
use std::process::{Command, ExitCode};
use wana_client::app::{App, Window};
use wana_log::{error, info, Subsystem};
use wana_text::bidi::Base;
use wana_text::font::Font;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::raster::{draw, Canvas};
use wana_text::fonts;

const LOG: Subsystem = Subsystem::Shell;
const WIDTH: u32 = 620;
const HEIGHT: u32 = 360;
const KEY_ESC: u32 = 1;
const KEY_ENTER: u32 = 28;
const KEY_UP: u32 = 103;
const KEY_DOWN: u32 = 108;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    PowerOff,
    Reboot,
    Cancel,
}

impl Choice {
    const ALL: [Self; 3] = [Self::PowerOff, Self::Reboot, Self::Cancel];

    const fn label(self) -> &'static str {
        match self {
            Self::PowerOff => "إيقاف التشغيل",
            Self::Reboot => "إعادة التشغيل",
            Self::Cancel => "إلغاء",
        }
    }

    const fn command(self) -> Option<&'static str> {
        match self {
            Self::PowerOff => Some("poweroff"),
            Self::Reboot => Some("reboot"),
            Self::Cancel => None,
        }
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
    let shaped = layout(value, fonts, &style)?;
    draw(c, &shaped, fonts, size, 40.0, y, rgb);
    Ok(())
}

fn render(fonts: &FontSet, selected: usize, confirm: bool) -> Result<Canvas, String> {
    let palette = wana_theme::current();
    let mut canvas = Canvas::new(WIDTH, HEIGHT, palette.bg);
    label(&mut canvas, fonts, "الطاقة", 24.0, 30.0, palette.text)?;
    label(
        &mut canvas,
        fonts,
        if confirm {
            "اضغط Enter مرة ثانية للتأكيد أو Esc للرجوع"
        } else {
            "اختر الإجراء — لن يتم التنفيذ بدون تأكيد"
        },
        66.0,
        16.0,
        if confirm { palette.danger } else { palette.dim },
    )?;

    for (index, choice) in Choice::ALL.iter().enumerate() {
        let y = 116 + index as u32 * 66;
        let active = index == selected;
        let background = if active {
            if confirm && choice.command().is_some() {
                palette.danger_strong
            } else {
                palette.accent
            }
        } else {
            palette.card
        };
        fill(&mut canvas, 36, y, WIDTH - 72, 54, background);
        label(
            &mut canvas,
            fonts,
            choice.label(),
            y as f32 + 13.0,
            19.0,
            if active {
                wana_theme::color::TEXT_DARK
            } else {
                palette.text
            },
        )?;
    }
    Ok(canvas)
}

fn execute(choice: Choice) -> Result<(), String> {
    let Some(action) = choice.command() else {
        return Ok(());
    };
    info!(LOG, "power UI confirmed action={action}");
    let status = Command::new("/usr/bin/wana-power")
        .arg(action)
        .status()
        .map_err(|e| format!("wana-power {action}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wana-power {action} exited {status}"))
    }
}

fn run() -> Result<(), String> {
    let font_dir = PathBuf::from(fonts::DEFAULT_DIR);
    fonts::verify_dir(&font_dir)?;
    let fonts = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|name| Font::load(&font_dir.join(name)))
            .collect::<Result<_, _>>()?,
    };

    let app = App::connect()?;
    let keyboard = app.keyboard()?.ok_or("power UI requires a keyboard seat")?;
    let window = Window::new(
        &app,
        "الطاقة — وانا",
        "org.wana.Power",
        WIDTH as i32,
        HEIGHT as i32,
    )?;

    let mut selected = 0usize;
    let mut confirm = false;
    let mut canvas = render(&fonts, selected, confirm)?;
    window.present(&app, &canvas.bytes())?;
    info!(LOG, "power UI mapped: {}x{}", WIDTH, HEIGHT);

    loop {
        app.conn.dispatch()?;
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
                    KEY_UP if !confirm => {
                        selected = selected.saturating_sub(1);
                        redraw = true;
                    }
                    KEY_DOWN if !confirm => {
                        selected = (selected + 1).min(Choice::ALL.len() - 1);
                        redraw = true;
                    }
                    KEY_ENTER if confirm => {
                        let choice = Choice::ALL[selected];
                        execute(choice)?;
                        if choice == Choice::Cancel {
                            window.destroy(&app);
                            return Ok(());
                        }
                    }
                    KEY_ENTER => {
                        let choice = Choice::ALL[selected];
                        if choice == Choice::Cancel {
                            window.destroy(&app);
                            return Ok(());
                        }
                        confirm = true;
                        redraw = true;
                        info!(LOG, "power UI confirmation requested action={}", choice.command().unwrap());
                    }
                    KEY_ESC if confirm => {
                        confirm = false;
                        redraw = true;
                    }
                    KEY_ESC => {
                        window.destroy(&app);
                        return Ok(());
                    }
                    _ => {}
                }
                if redraw {
                    canvas = render(&fonts, selected, confirm)?;
                    window.present(&app, &canvas.bytes())?;
                }
            }
            app.protocol_event(&event)?;
        }
    }
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "power UI: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_choices_require_commands_but_cancel_does_not() {
        assert_eq!(Choice::PowerOff.command(), Some("poweroff"));
        assert_eq!(Choice::Reboot.command(), Some("reboot"));
        assert_eq!(Choice::Cancel.command(), None);
    }
}
