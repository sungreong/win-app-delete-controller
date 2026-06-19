use std::fs;
use std::sync::Arc;

use eframe::egui::{Context, FontData, FontDefinitions, FontFamily};

const KOREAN_FONT_NAME: &str = "MalgunGothic";
const KOREAN_FONT_PATHS: &[&str] = &[
    r"C:\Windows\Fonts\malgun.ttf",
    r"C:\Windows\Fonts\malgunbd.ttf",
    r"C:\Windows\Fonts\gulim.ttc",
];

pub fn configure_korean_fonts(ctx: &Context) {
    let Some(font_bytes) = load_first_available_font() else {
        return;
    };

    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        KOREAN_FONT_NAME.to_owned(),
        Arc::new(FontData::from_owned(font_bytes)),
    );

    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, KOREAN_FONT_NAME.to_owned());
    }

    ctx.set_fonts(fonts);
}

fn load_first_available_font() -> Option<Vec<u8>> {
    KOREAN_FONT_PATHS
        .iter()
        .find_map(|path| fs::read(path).ok())
}
