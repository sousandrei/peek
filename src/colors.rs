use anstyle::{Ansi256Color, AnsiColor, Color as AnsiStyleColor, RgbColor, Style as AnsiStyle};
use ratatui::style::Color as TuiColor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColorRole {
    Background,
    Panel,
    Text,
    Muted,
    Border,
    Accent,
    Selection,
    Added,
    Modified,
    Removed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColorDepth {
    None,
    Ansi16,
    Ansi256,
    TrueColor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Colors {
    depth: ColorDepth,
}

impl Colors {
    pub fn detect(is_terminal: bool) -> Self {
        Self::from_capabilities(
            is_terminal,
            std::env::var_os("NO_COLOR").is_some(),
            std::env::var_os("CI").is_some(),
            std::env::var("TERM").ok().as_deref(),
            std::env::var("COLORTERM").ok().as_deref(),
        )
    }

    fn from_capabilities(
        is_terminal: bool,
        no_color: bool,
        ci: bool,
        term: Option<&str>,
        color_term: Option<&str>,
    ) -> Self {
        let depth = if !is_terminal || no_color || ci || term == Some("dumb") {
            ColorDepth::None
        } else if color_term.is_some_and(|value| {
            value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
        }) || term
            .is_some_and(|value| value.contains("truecolor") || value.ends_with("-direct"))
        {
            ColorDepth::TrueColor
        } else if term.is_some_and(|value| value.contains("256color")) {
            ColorDepth::Ansi256
        } else {
            ColorDepth::Ansi16
        };

        Self { depth }
    }

    pub fn is_enabled(self) -> bool {
        self.depth != ColorDepth::None
    }

    pub fn color_choice(self) -> anstream::ColorChoice {
        if self.is_enabled() {
            anstream::ColorChoice::Auto
        } else {
            anstream::ColorChoice::Never
        }
    }

    pub fn tui(self, role: ColorRole) -> TuiColor {
        let rgb = rgb(role);
        match self.depth {
            ColorDepth::None => TuiColor::Reset,
            ColorDepth::Ansi16 => TuiColor::Indexed(Ansi256Color::from_ansi(ansi16(role)).index()),
            ColorDepth::Ansi256 => TuiColor::Indexed(ansi256(rgb).index()),
            ColorDepth::TrueColor => TuiColor::Rgb(rgb.r(), rgb.g(), rgb.b()),
        }
    }

    pub fn ansi(self, role: ColorRole) -> AnsiStyle {
        let color = match self.depth {
            ColorDepth::None => return AnsiStyle::new(),
            ColorDepth::Ansi16 => AnsiStyleColor::Ansi(ansi16(role)),
            ColorDepth::Ansi256 => AnsiStyleColor::Ansi256(ansi256(rgb(role))),
            ColorDepth::TrueColor => AnsiStyleColor::Rgb(rgb(role)),
        };

        AnsiStyle::new().fg_color(Some(color))
    }
}

fn rgb(role: ColorRole) -> RgbColor {
    match role {
        ColorRole::Background => RgbColor(14, 18, 26),
        ColorRole::Panel => RgbColor(20, 25, 35),
        ColorRole::Text => RgbColor(218, 226, 238),
        ColorRole::Muted => RgbColor(131, 147, 170),
        ColorRole::Border => RgbColor(49, 62, 82),
        ColorRole::Accent => RgbColor(110, 207, 229),
        ColorRole::Selection => RgbColor(36, 54, 74),
        ColorRole::Added => RgbColor(144, 211, 151),
        ColorRole::Modified => RgbColor(234, 192, 124),
        ColorRole::Removed => RgbColor(234, 143, 153),
    }
}

fn ansi16(role: ColorRole) -> AnsiColor {
    match role {
        ColorRole::Background | ColorRole::Panel => AnsiColor::Black,
        ColorRole::Text => AnsiColor::BrightWhite,
        ColorRole::Muted | ColorRole::Border => AnsiColor::BrightBlack,
        ColorRole::Accent => AnsiColor::BrightCyan,
        ColorRole::Selection => AnsiColor::BrightBlue,
        ColorRole::Added => AnsiColor::BrightGreen,
        ColorRole::Modified => AnsiColor::BrightYellow,
        ColorRole::Removed => AnsiColor::BrightRed,
    }
}

fn ansi256(color: RgbColor) -> Ansi256Color {
    let red = color.r();
    let green = color.g();
    let blue = color.b();
    let spread = red.max(green).max(blue) - red.min(green).min(blue);

    if spread < 18 {
        let average = (u16::from(red) + u16::from(green) + u16::from(blue)) / 3;
        let level = ((average.saturating_sub(8) + 5) / 10).min(23) as u8;
        return Ansi256Color(232 + level);
    }

    let red = color_cube_level(red);
    let green = color_cube_level(green);
    let blue = color_cube_level(blue);
    Ansi256Color(16 + 36 * red + 6 * green + blue)
}

fn color_cube_level(value: u8) -> u8 {
    match value {
        0..=47 => 0,
        48..=114 => 1,
        115..=154 => 2,
        155..=194 => 3,
        195..=234 => 4,
        _ => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorDepth, ColorRole, Colors, ansi256, rgb};
    use anstyle::Ansi256Color;

    #[test]
    fn detects_color_depth_and_respects_plain_output_settings() {
        assert_eq!(
            Colors::from_capabilities(true, false, false, Some("xterm-256color"), None).depth,
            ColorDepth::Ansi256
        );
        assert_eq!(
            Colors::from_capabilities(true, false, false, None, Some("TrueColor")).depth,
            ColorDepth::TrueColor
        );
        assert_eq!(
            Colors::from_capabilities(true, true, false, Some("xterm-256color"), None).depth,
            ColorDepth::None
        );
        assert_eq!(
            Colors::from_capabilities(true, false, true, Some("xterm-256color"), None).depth,
            ColorDepth::None
        );
        assert_eq!(
            Colors::from_capabilities(true, false, false, Some("dumb"), None).depth,
            ColorDepth::None
        );
    }

    #[test]
    fn maps_semantic_palette_colors_to_xterm_256_indices() {
        assert_eq!(ansi256(rgb(ColorRole::Background)), Ansi256Color(233));
        assert_eq!(ansi256(rgb(ColorRole::Accent)), Ansi256Color(80));
    }

    #[test]
    fn terminal_styles_use_the_same_semantic_rgb_palette() {
        let colors = Colors {
            depth: ColorDepth::TrueColor,
        };

        assert_eq!(
            colors.tui(ColorRole::Accent),
            ratatui::style::Color::Rgb(110, 207, 229)
        );
        assert_eq!(
            colors.ansi(ColorRole::Accent),
            anstyle::RgbColor(110, 207, 229).on_default()
        );
    }
}
