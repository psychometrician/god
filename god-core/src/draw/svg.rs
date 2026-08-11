//! A scene drawn as a picture.
//!
//! **This module is the only place in the drawing that knows what a pixel is.**
//! Everything above it works in cells of a fixed grid, so what happens here is
//! multiplication and nothing else: a column becomes an `x`, a line becomes a
//! `y`, and the layout was already settled by whoever built the scene. That is
//! what makes the ladder in a terminal a proof of the picture — they are the
//! same positions, read twice.
//!
//! Five rules hold the output steady, and each one closes a way that generated
//! files stop being identical:
//!
//! - **Whole numbers only.** No float is formatted anywhere here, so there is no
//!   rounding for two runs to disagree about.
//! - **Nothing is named.** No generated `id`, no timestamp, no counter. There is
//!   nothing in the file whose value depends on when it was written.
//! - **One escape.** [`Svg::out`] is private and [`Svg::text`] is the only thing
//!   that writes a `<`, so no caller can put an unescaped column name into the
//!   markup.
//! - **No inline style.** Every mark carries a class from [`Ink::class`], and
//!   the stylesheet defines all of them — checked by walking [`Ink::ALL`]
//!   rather than by reading.
//! - **Every run is pinned.** `textLength` tells the renderer exactly how wide a
//!   run must be, so a machine whose monospace font is not the one this was
//!   written on still lines its columns up. Without it the grid is a guess about
//!   somebody else's font.
//!
//! **A picture rather than a page of HTML, on purpose.** A table of `<div>`s
//! would let a browser do the text metrics and would also let the page it lands
//! on restyle it: a theme, a notebook, a reader's own sheet all get a vote.
//! Attributes here are sealed, so the same sentence is the same picture wherever
//! it is opened — which is the whole reason to draw it.

use super::scene::{cells, Ink, Scene};

/// One character wide.
const CH_W: u16 = 10;
/// One line tall.
const LINE_H: u16 = 24;
/// The margin, left and right.
const PAD_X: u16 = 18;
/// The margin, top and bottom.
const PAD_Y: u16 = 16;
/// Where the text sits inside its line.
const BASELINE: u16 = 17;
/// Chosen against `CH_W` so the pinning barely stretches anything: the monospace
/// faces named in the stylesheet advance 0.6em, which is 9.6 at this size.
const FONT_PX: u16 = 16;

/// The scene as a standalone SVG document.
pub fn render(scene: &Scene) -> String {
    let width = PAD_X * 2 + scene.width * CH_W;
    let height = PAD_Y * 2 + scene.rows.len() as u16 * LINE_H;

    let mut svg = Svg { out: String::new() };
    svg.open(width, height);
    svg.style();
    svg.raw(&format!(
        "<rect class=\"ground\" x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\"/>"
    ));

    shade_bands(&mut svg, scene, width);

    for (line, row) in scene.rows.iter().enumerate() {
        let line = line as u16;
        for cell in &row.cells {
            if cell.ink == Ink::Rail {
                rail(&mut svg, &cell.text, cell.col, line);
            } else {
                svg.text(cell.col, line, &cell.text, cell.ink);
            }
        }
    }

    svg.raw("</svg>");
    svg.out
}

/// A faint ground under alternate steps, so the lines belonging to one step read
/// as one thing. A band can be four lines deep — its columns, what it dropped, a
/// table arriving, what became of the rows — and without this they run together.
fn shade_bands(svg: &mut Svg, scene: &Scene, width: u16) {
    let mut line = 0usize;
    while line < scene.rows.len() {
        let band = scene.rows[line].band;
        let mut end = line;
        while end < scene.rows.len() && scene.rows[end].band == band {
            end += 1;
        }
        if band % 2 == 1 {
            let y = PAD_Y + line as u16 * LINE_H;
            let h = (end - line) as u16 * LINE_H;
            svg.raw(&format!(
                "<rect class=\"band\" x=\"0\" y=\"{y}\" width=\"{width}\" height=\"{h}\"/>"
            ));
        }
        line = end;
    }
}

/// The spine, drawn rather than typed.
///
/// **The three glyphs the ladder builds its rail from are structure, not text.**
/// Set as characters they would leave a hairline gap at every line boundary,
/// because a glyph is shorter than the line it sits in; as strokes they join.
/// Anything else with this ink is drawn as text, and a check walks the corpus to
/// make sure the ladder never invents a fourth.
fn rail(svg: &mut Svg, glyph: &str, col: u16, line: u16) {
    let x = PAD_X + col * CH_W + CH_W / 2;
    let top = PAD_Y + line * LINE_H;
    let mid = top + LINE_H / 2;
    let bottom = top + LINE_H;
    let right = x + CH_W;

    let path = match glyph {
        "│" => format!("M{x} {top}V{bottom}"),
        "├" => format!("M{x} {top}V{bottom}M{x} {mid}H{right}"),
        "└" => format!("M{x} {top}V{mid}H{right}"),
        // A space holds the column open where a rail has ended. Nothing to draw.
        _ => return,
    };
    svg.raw(&format!("<path class=\"rail\" d=\"{path}\"/>"));
}

/// Whether this emitter knows how to draw a piece of rail.
///
/// Public so a test can walk every drawing the corpus produces and ask, rather
/// than a reader having to notice that a new glyph came out as nothing.
pub fn draws_rail(glyph: &str) -> bool {
    matches!(glyph, "│" | "├" | "└" | " ")
}

struct Svg {
    out: String,
}

impl Svg {
    fn raw(&mut self, markup: &str) {
        self.out.push_str(markup);
        self.out.push('\n');
    }

    fn open(&mut self, width: u16, height: u16) {
        self.raw(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
             viewBox=\"0 0 {width} {height}\">"
        ));
    }

    /// One run of text, pinned to the width the grid gave it.
    ///
    /// The only place a `<` is written from something a caller supplied, which is
    /// why the escaping lives here and the buffer is private: a column called
    /// `a<b` cannot reach the markup any other way.
    fn text(&mut self, col: u16, line: u16, body: &str, ink: Ink) {
        let x = PAD_X + col * CH_W;
        let y = PAD_Y + line * LINE_H + BASELINE;
        let length = cells(body) * CH_W;
        let class = ink.class();
        self.raw(&format!(
            "<text class=\"{class}\" x=\"{x}\" y=\"{y}\" textLength=\"{length}\" \
             lengthAdjust=\"spacingAndGlyphs\">{}</text>",
            escape(body)
        ));
    }

    /// **Every class in one block, and the block is checked against `Ink::ALL`.**
    ///
    /// The dark half is a media query inside the file rather than something the
    /// page decides, so the picture answers to the reader's own setting and to
    /// nothing else that happens to be on the page with it.
    fn style(&mut self) {
        self.raw(&format!(
            "<style>\
             text{{font-family:ui-monospace,SFMono-Regular,'SF Mono',Menlo,Consolas,'DejaVu Sans Mono',monospace;font-size:{FONT_PX}px;white-space:pre}}\
             .ground{{fill:#fbfaf8}}\
             .band{{fill:#1a1a18;opacity:.028}}\
             .rail{{stroke:#cbc7bf;stroke-width:1.5;fill:none;stroke-linecap:square}}\
             .source{{fill:#1a1a18;font-weight:600}}\
             .step{{fill:#26251f}}\
             .table{{fill:#1a1a18;font-weight:600}}\
             .column{{fill:#57544c}}\
             .kind{{fill:#a5a096}}\
             .added{{fill:#1c7a4a;font-weight:600}}\
             .dropped{{fill:#a8514a;text-decoration:line-through}}\
             .key{{fill:#3062a8;font-weight:600}}\
             .note{{fill:#8b867e}}\
             .warn{{fill:#8a6410}}\
             .caret{{fill:#a8514a;font-weight:600}}\
             @media (prefers-color-scheme:dark){{\
             .ground{{fill:#17181b}}\
             .band{{fill:#ffffff;opacity:.035}}\
             .rail{{stroke:#3d4046}}\
             .source{{fill:#eceae5}}\
             .step{{fill:#dedbd4}}\
             .table{{fill:#eceae5}}\
             .column{{fill:#b3afa6}}\
             .kind{{fill:#6e6a63}}\
             .added{{fill:#5cc78c}}\
             .dropped{{fill:#e2908a}}\
             .key{{fill:#7fabf2}}\
             .note{{fill:#7e7a73}}\
             .warn{{fill:#d7a54c}}\
             .caret{{fill:#e2908a}}\
             }}</style>"
        ));
    }
}

/// The five characters that cannot stand for themselves in markup.
///
/// A column name may hold any of them — the grammar puts a name in brackets and
/// asks nothing else of it — so this is not a formality.
fn escape(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for c in body.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ink_has_a_class_in_the_stylesheet() {
        let mut svg = Svg { out: String::new() };
        svg.style();
        for ink in Ink::ALL {
            assert!(
                svg.out.contains(&format!(".{}{{", ink.class())),
                "`{}` is a kind of ink with no rule in the stylesheet, so it would \
                 be drawn as whatever the default happens to be",
                ink.class()
            );
        }
    }

    #[test]
    fn a_column_name_cannot_reach_the_markup() {
        assert_eq!(escape("a<b & c\"d"), "a&lt;b &amp; c&quot;d");
    }
}
