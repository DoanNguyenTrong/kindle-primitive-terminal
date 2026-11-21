//! Draws the terminal into an 8-bit gray buffer: shapes with `vello_cpu`, text with `parley` (the same libraries
//! and versions as Primitive Browser and Launcher). Chrome and text are pure black on pure white after
//! `solidify`, so a page change does not flash; the keyboard uses the shared look (`primitive-keyboard`).

use kt_core::{BarKey, Rect, Terminal, PROMPT};
use parley::{FontContext, FontWeight, LayoutContext, PositionedLayoutItem, StyleProperty};
use primitive_keyboard::{style, Key, KeyDef};
use vello_cpu::{
    kurbo::{self, Affine, Shape},
    peniko::Color,
    Glyph, Pixmap, RenderContext, Resources,
};

const BLACK: u8 = 0;
const WHITE: u8 = 255;
const MARGIN_X: i32 = 40;
/// The output's font size and the height of one of its lines.
const OUTPUT_SIZE: f32 = 30.0;
const LINE_H: i32 = 38;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Start,
    Center,
}

struct Text<'a> {
    text: &'a str,
    x: f32,
    y: f32,
    w: f32,
    size: f32,
    bold: bool,
    align: Align,
    gray: u8,
}

fn gray(v: u8) -> Color {
    Color::from_rgb8(v, v, v)
}

fn krect(r: Rect) -> kurbo::Rect {
    kurbo::Rect::new(r.x as f64, r.y as f64, (r.x + r.w) as f64, (r.y + r.h) as f64)
}

/// Make the panel's near-black and near-white pure, so text and chrome change only pure pixels.
pub fn solidify(gray: &mut [u8]) {
    for px in gray.iter_mut() {
        if *px <= 34 {
            *px = 0;
        } else if *px >= 221 {
            *px = 255;
        }
    }
}

pub struct Renderer {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
    fonts: FontContext,
    layout_cx: LayoutContext<()>,
}

impl Renderer {
    /// A renderer for a `width` × `height` screen, with the Noto Sans fonts at `font_paths`. Fails when no font loads.
    pub fn new(width: u32, height: u32, font_paths: &[String]) -> Result<Renderer, String> {
        let mut fonts = FontContext::new();
        let existing: Vec<&String> = font_paths.iter().filter(|p| std::fs::metadata(p).is_ok()).collect();
        if existing.is_empty() {
            return Err(format!("no fonts found in {font_paths:?}"));
        }
        fonts.collection.load_fonts_from_paths(existing.iter().map(|p| p.as_str()));
        let Some(family) = fonts.collection.family_id("Noto Sans") else {
            return Err("font family Noto Sans not found".into());
        };
        fonts.collection.set_generic_families(parley::GenericFamily::SansSerif, std::iter::once(family));
        let (w, h) = (width as u16, height as u16);
        Ok(Renderer {
            ctx: RenderContext::new(w, h),
            resources: Resources::new(),
            pixmap: Pixmap::new(w, h),
            fonts,
            layout_cx: LayoutContext::new(),
        })
    }

    // ----- primitives -----

    fn fill(&mut self, r: Rect, v: u8) {
        self.ctx.set_transform(Affine::IDENTITY);
        self.ctx.set_paint(gray(v));
        self.ctx.fill_rect(&krect(r));
    }

    fn rounded(&mut self, r: Rect, radius: f64, v: u8) {
        self.ctx.set_transform(Affine::IDENTITY);
        self.ctx.set_paint(gray(v));
        let path = kurbo::RoundedRect::from_rect(krect(r), radius).to_path(0.1);
        self.ctx.fill_path(&path);
    }

    /// A rounded outline: an outer fill in `line`, an inner fill in `inside`.
    fn rounded_frame(&mut self, r: Rect, radius: f64, width: i32, line: u8, inside: u8) {
        self.rounded(r, radius, line);
        let inner = Rect { x: r.x + width, y: r.y + width, w: r.w - 2 * width, h: r.h - 2 * width };
        self.rounded(inner, (radius - width as f64).max(0.0), inside);
    }

    fn text(&mut self, t: Text) -> f32 {
        let layout = self.layout_text(t.text, t.size, t.bold, t.w);
        let Some(line) = layout.lines().next() else { return 0.0 };
        let m = line.metrics();
        let dx = match t.align {
            Align::Start => 0.0,
            Align::Center => ((t.w - m.advance) / 2.0).max(0.0),
        };
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
            let run = glyph_run.run();
            self.ctx.set_transform(Affine::translate(((t.x + dx) as f64, t.y as f64)));
            self.ctx.set_paint(gray(t.gray));
            let _ = self
                .ctx
                .glyph_run(&mut self.resources, run.font())
                .font_size(run.font_size())
                .hint(true)
                .normalized_coords(&[])
                .fill_glyphs(glyph_run.positioned_glyphs().map(|g| Glyph { id: g.id, x: g.x, y: g.y }));
        }
        self.ctx.set_transform(Affine::IDENTITY);
        m.line_height
    }

    fn layout_text(&mut self, text: &str, size: f32, bold: bool, w: f32) -> parley::Layout<()> {
        let mut builder = self.layout_cx.ranged_builder(&mut self.fonts, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(size));
        builder.push_default(StyleProperty::FontWeight(if bold { FontWeight::BOLD } else { FontWeight::NORMAL }));
        builder.push_default(StyleProperty::LineHeight(parley::LineHeight::FontSizeRelative(1.25)));
        let mut layout: parley::Layout<()> = builder.build(text);
        layout.break_all_lines(Some(w));
        layout
    }

    /// The width of one line of text, in pixels.
    fn text_width(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        let layout = self.layout_text(text, size, bold, f32::MAX);
        let width = layout.lines().next().map_or(0.0, |l| l.metrics().advance);
        width
    }

    /// Split one output line into the rows it takes at `w` pixels: words are kept together where they fit, and a
    /// word wider than the screen is cut by character.
    fn wrap(&mut self, line: &str, w: f32) -> Vec<String> {
        let mut rows: Vec<String> = Vec::new();
        let mut cur = String::new();
        for token in line.split_inclusive(' ') {
            let joined = format!("{cur}{token}");
            if self.text_width(&joined, OUTPUT_SIZE, false) <= w {
                cur = joined;
                continue;
            }
            if !cur.is_empty() {
                rows.push(cur.trim_end().to_string());
                cur = String::new();
            }
            if self.text_width(token, OUTPUT_SIZE, false) <= w {
                cur = token.to_string();
                continue;
            }
            for c in token.chars() {
                let next = format!("{cur}{c}");
                if !cur.is_empty() && self.text_width(&next, OUTPUT_SIZE, false) > w {
                    rows.push(std::mem::take(&mut cur));
                    cur = c.to_string();
                } else {
                    cur = next;
                }
            }
        }
        rows.push(cur.trim_end().to_string());
        rows
    }

    // ----- screen -----

    /// Draw the terminal. Returns `width × height` gray pixels, upright.
    pub fn draw(&mut self, t: &Terminal) -> Vec<u8> {
        self.ctx.reset();
        self.fill(Rect { x: 0, y: 0, w: t.width, h: t.height }, WHITE);
        self.title(t);
        self.output(t);
        self.input(t);
        self.key_bar(t);
        self.keyboard(t);
        self.ctx.flush();
        self.ctx.render(self.pixmap.as_mut(), &mut self.resources);
        let mut out: Vec<u8> = self.pixmap.data_as_u8_slice().iter().step_by(4).copied().collect();
        solidify(&mut out);
        // Drawn upright. The device turns the picture when it is flipped (see the terminal's main loop).
        out
    }

    fn title(&mut self, t: &Terminal) {
        self.text(Text { text: "Terminal", x: 40.0, y: 30.0, w: 600.0, size: 56.0, bold: true, align: Align::Start, gray: BLACK });
        let back = t.back_button();
        self.button(back, "Exit", true);
        let bar = t.title_bar();
        self.fill(Rect { x: 0, y: bar.bottom() - 4, w: bar.w, h: 4 }, BLACK);
    }

    fn button(&mut self, r: Rect, label: &str, primary: bool) {
        let (fill, ink) = if primary { (BLACK, WHITE) } else { (WHITE, BLACK) };
        self.rounded_frame(r, 14.0, 3, BLACK, fill);
        self.text(Text { text: label, x: r.x as f32, y: r.y as f32 + 22.0, w: r.w as f32, size: 34.0, bold: true, align: Align::Center, gray: ink });
    }

    /// The output: the newest rows that fit, above the input line. The scroll position is applied by the terminal.
    fn output(&mut self, t: &Terminal) {
        let area = t.output_area();
        let max_rows = (area.h / LINE_H).max(1) as usize;
        let text_w = (area.w - 2 * MARGIN_X) as f32;
        let mut rows: Vec<String> = Vec::new();
        // Walk back from the newest line until the rows that fit are found.
        let visible = t.visible_lines();
        for line in visible.iter().rev() {
            let mut wrapped = self.wrap(line, text_w);
            wrapped.append(&mut rows);
            rows = wrapped;
            if rows.len() >= max_rows {
                break;
            }
        }
        let start = rows.len().saturating_sub(max_rows);
        for (i, row) in rows[start..].iter().enumerate() {
            let y = area.y + 8 + i as i32 * LINE_H;
            self.text(Text { text: row, x: MARGIN_X as f32, y: y as f32, w: text_w, size: OUTPUT_SIZE, bold: false, align: Align::Start, gray: BLACK });
        }
    }

    /// The command being typed, after the prompt. A long command shows its end, where the caret is.
    fn input(&mut self, t: &Terminal) {
        let line = t.input_line();
        self.fill(Rect { x: 0, y: line.y, w: line.w, h: 2 }, 226);
        let prompt_w = self.text_width(PROMPT, 34.0, true);
        let y = line.y as f32 + 26.0;
        self.text(Text { text: PROMPT, x: MARGIN_X as f32, y, w: prompt_w + 4.0, size: 34.0, bold: true, align: Align::Start, gray: BLACK });
        let avail = (line.w - 2 * MARGIN_X) as f32 - prompt_w - 24.0;
        let mut shown: String = t.command.text.iter().collect();
        while !shown.is_empty() && self.text_width(&shown, 34.0, false) > avail {
            shown.remove(0);
        }
        let x = MARGIN_X as f32 + prompt_w;
        self.text(Text { text: &shown, x, y, w: avail + 20.0, size: 34.0, bold: false, align: Align::Start, gray: BLACK });
        let caret_x = x + self.text_width(&shown, 34.0, false) + 2.0;
        self.fill(Rect { x: caret_x as i32, y: line.y + 22, w: 3, h: 46 }, BLACK);
    }

    /// The key bar above the keyboard: Ctrl (filled while it is on), Up, Down and Stop, in the keyboard's key look.
    fn key_bar(&mut self, t: &Terminal) {
        let bar = t.key_bar();
        self.fill(Rect { x: 0, y: bar.y, w: bar.w, h: 2 }, 226);
        for (key, r) in t.bar_keys() {
            let (label, on) = match key {
                BarKey::Ctrl => ("Ctrl", t.ctrl),
                BarKey::Up => ("Up", false),
                BarKey::Down => ("Down", false),
                BarKey::Stop => ("Stop", false),
            };
            let def = KeyDef { key: Key::Space, label: label.to_string(), width: 1.0, on };
            let look = primitive_keyboard::look(&def, t.pressed == Some(r));
            self.rounded_frame(r, style::RADIUS as f64, style::BORDER as i32, look.line, look.fill);
            self.text(Text { text: label, x: r.x as f32, y: r.y as f32 + r.h as f32 / 2.0 - 22.0, w: r.w as f32, size: 34.0, bold: true, align: Align::Center, gray: look.ink });
        }
    }

    fn keyboard(&mut self, t: &Terminal) {
        let area = t.keyboard_area();
        self.fill(Rect { x: 0, y: area.y, w: area.w, h: style::TOP_LINE as i32 }, style::TOP_LINE_GRAY);
        for (def, r) in t.keys() {
            let pressed = t.pressed == Some(r);
            self.key(&def, r, pressed);
        }
    }

    /// One key, in the keyboard's shared look (`primitive_keyboard::look`): on, or pressed, takes the ink look.
    fn key(&mut self, def: &KeyDef, r: Rect, pressed: bool) {
        let look = primitive_keyboard::look(def, pressed);
        self.rounded_frame(r, style::RADIUS as f64, style::BORDER as i32, look.line, look.fill);
        let size = style::label_size(&def.label) as f32;
        self.text(Text { text: &def.label, x: r.x as f32, y: r.y as f32 + r.h as f32 / 2.0 - size * 0.7, w: r.w as f32, size, bold: false, align: Align::Center, gray: look.ink });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solidify_makes_near_black_and_near_white_pure() {
        let mut px = vec![0, 20, 34, 35, 128, 220, 221, 255];
        solidify(&mut px);
        assert_eq!(px, vec![0, 0, 0, 35, 128, 220, 255, 255]);
    }
}
