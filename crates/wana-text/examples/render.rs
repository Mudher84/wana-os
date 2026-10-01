use wana_text::bidi::Base;
use wana_text::font::Font;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::raster::{draw, Canvas};
fn main() {
    let dir = std::path::Path::new("out/fonts");
    let fonts = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|n| Font::load(&dir.join(n)).unwrap())
            .collect(),
    };
    let size = 40.0;
    let st = Style {
        size,
        base: Base::Auto,
        align: Align::Start,
        width: Some(560.0),
        language: "ar".into(),
    };
    let l = layout(
        "مرحبا بك في وانا، نظام تشغيل مستقل Wana OS 2026",
        &fonts,
        &st,
    )
    .unwrap();
    let mut c = Canvas::new(600, (l.height + 40.0) as u32, 0x16213e);
    draw(&mut c, &l, &fonts, size, 20.0, 20.0, 0xFFFFFF);
    let mut ppm = format!("P6\n{} {}\n255\n", c.width, c.height).into_bytes();
    for p in &c.pixels {
        ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(std::env::args().nth(1).unwrap(), ppm).unwrap();
}
