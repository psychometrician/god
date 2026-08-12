//! A pipeline drawn as a diagram rather than set as text.
//!
//! **This is the emitter that stopped being a picture of a terminal.** The
//! ladder and the first SVG were one layout in two media: the same characters
//! on a grid, one of them colored. That is honest and it is not a diagram, and
//! on a page it reads as a screenshot of a console.
//!
//! What a diagram can say that a line of text cannot is *shape*. A table is
//! drawn as a bar holding one chip per column, so its width is its width: a
//! `summarize` visibly narrows the page, a `join` visibly widens it, and a
//! reader sees what happened before reading a word. A table arriving is a box
//! off to the side with an arrow into the step that consumes it, and the
//! columns that cross appear as the same chips at both ends.
//!
//! **It shares the reading and nothing else.** What a step makes, what it takes
//! away, which table arrives and what became of the rows are worked out once, by
//! the ladder's own semantics; only the placing is here. That is the line the
//! two drawings must not cross, because two readings of one sentence is how they
//! start disagreeing about it.
//!
//! **Metrics stay exact without a character grid.** Column names are code and
//! are set in a monospace face, so a run of `n` characters is `n * CH` wide and
//! a chip is that plus its padding. Nothing here measures a proportional font,
//! nothing guesses, and every coordinate is a whole number — so the same
//! sentence draws the same bytes, which is what the ladder's rules bought and
//! this keeps.

use super::ladder::{
    chips_between, chips_of, chips_from, rows_notes, tributaries, Arriving, Chip, Reading,
};
use super::scene::Ink;
use crate::backend::god::step_text;
use crate::check::{Schema, Tables};

/// One character of the monospace face, at `FONT`. Chosen so the two multiply
/// to a whole number: the faces named in the stylesheet advance 0.6em.
const CH: u32 = 9;
const FONT: u32 = 15;
/// The smaller face, for what a column holds and for the notes.
const SMALL: u32 = 12;
const SMALL_CH: u32 = 7;

const PAD: u32 = 26;
/// The spine, and how far the step's words sit from it.
const RAIL_X: u32 = PAD + 8;
const LABEL_X: u32 = RAIL_X + 22;
/// How many characters of a step's words go on one line. Beyond this they fold,
/// for the reason the chips wrap: an `across` written back out is one assignment
/// per column, and one line of that is wider than any page.
const LABEL_CHARS: usize = 46;
/// One line of folded words.
const LINE_H: u32 = 20;
/// How many chips go in one row of a bar. Fewer than the ladder's, because a
/// drawn chip is wider than a name and a picture has a page to stay inside.
const WRAP: usize = 5;

const CHIP_H: u32 = 26;
const CHIP_PAD: u32 = 9;
const CHIP_GAP: u32 = 6;
const BAR_PAD: u32 = 8;
const BAR_H: u32 = CHIP_H + BAR_PAD * 2;

const ROW_GAP: u32 = 10;
const NOTE_H: u32 = 18;

/// How many chips a bar shows before it counts instead. A wide table is drawn
/// as its first columns and a tally, never as a bar off the side of the page.
const CHIP_CAP: usize = 10;

pub fn render(seen: &Reading, others: &Tables) -> String {
    let mut d = Draw { out: String::new(), width: 0, height: 0 };

    // Everything is laid out into a buffer first, because the document's size is
    // not known until the last band is placed and an SVG declares it up front.
    let mut body = String::new();
    let mut y = PAD;

    let labels: Vec<String> = seen.plan.steps.iter().map(step_text).collect();
    let widest = labels
        .iter()
        .map(|l| super::scene::cells(l) as usize)
        .chain(std::iter::once(super::scene::cells(&seen.plan.source) as usize))
        .max()
        .unwrap_or(0)
        .min(LABEL_CHARS);
    let bar_x = LABEL_X + widest as u32 * CH + 28;

    let mut rail_from = 0u32;
    let mut rail_to = 0u32;

    if !seen.states.is_empty() {
        // The table the sentence starts from. Its bar is the width to compare
        // every later one against.
        let top = y;
        d.node(&mut body, rail_x(), top + BAR_H / 2);
        d.label(&mut body, LABEL_X, top + BAR_H / 2, &seen.plan.source, Ink::Source, FONT);
        d.bar(&mut body, bar_x, top, &chips_of(&seen.states[0]), &mut y);
        rail_from = top + BAR_H / 2;
        rail_to = rail_from;
        y += ROW_GAP;

        for (i, step) in seen.plan.steps.iter().enumerate() {
            let arrivals = tributaries(step);
            let keys: Vec<String> = arrivals.iter().flat_map(|t| t.keys.clone()).collect();
            let (kept, gone) = chips_between(&seen.states[i], &seen.states[i + 1], &keys);

            let top = y;
            let middle = top + BAR_H / 2;
            d.node(&mut body, rail_x(), middle);
            d.label(&mut body, LABEL_X, middle, &labels[i], Ink::Step, FONT);
            d.bar(&mut body, bar_x, top, &kept, &mut y);
            rail_to = middle;

            // What left, under the bar it left from, struck through so the eye
            // reads it as gone rather than as more of the table.
            if !gone.is_empty() {
                y += 4;
                d.gone(&mut body, bar_x + BAR_PAD, &mut y, &gone);
            }

            // A table arriving, drawn where it arrives: indented under the step
            // that reads it, with an arrow up into that step's bar.
            for arrival in &arrivals {
                y += 6;
                let their = others.get(&arrival.other);
                let top_of_box = y;
                d.arrow(&mut body, bar_x + 14, top_of_box, middle + BAR_H / 2);
                d.label(&mut body, bar_x + 30, top_of_box + BAR_H / 2, &arrival.other, Ink::Table, FONT);
                let from = bar_x + 30 + text_w(&arrival.other, CH) + 16;
                match their {
                    Some(s) => {
                        d.bar(&mut body, from, top_of_box, &chips_from(s, &arrival.keys), &mut y)
                    }
                    None => y += BAR_H,
                }
                y += 2;
                d.note(&mut body, bar_x + 30, y + SMALL, &arrival.crossing(their), Ink::Note);
                y += NOTE_H;
            }

            for note in seen.assumptions.iter().filter(|a| a.span == Some(step.span())) {
                d.note(&mut body, bar_x, y + SMALL, &note.message, Ink::Note);
                y += NOTE_H;
            }
            for (text, ink) in rows_notes(step, &arrivals) {
                let ink = match arrivals.first().map(|a| &a.kind) {
                    Some(Arriving::Columns(_)) if ink == Ink::Warn => Ink::Warn,
                    _ => ink,
                };
                d.note(&mut body, bar_x, y + SMALL, &text, ink);
                y += NOTE_H;
            }
            y += ROW_GAP;
        }
    } else {
        d.label(&mut body, LABEL_X, y + BAR_H / 2, &seen.plan.source, Ink::Source, FONT);
        y += BAR_H + ROW_GAP;
    }

    if let Some((blame, refusal)) = &seen.refusal {
        let words = blame
            .and_then(|s| seen.source.get(s.start..s.start + s.len))
            .unwrap_or("")
            .trim();
        let middle = y + BAR_H / 2;
        d.cross(&mut body, rail_x(), middle);
        d.label(
            &mut body,
            LABEL_X,
            middle,
            if words.is_empty() { "this sentence" } else { words },
            Ink::Caret,
            FONT,
        );
        y += BAR_H;
        d.note(&mut body, LABEL_X, y + SMALL, &refusal.message, Ink::Warn);
        y += NOTE_H + ROW_GAP;
        rail_to = middle;
    }

    let height = y + PAD;
    let width = d.width + PAD;
    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\">\n"
    ));
    out.push_str(&style());
    out.push_str(&format!(
        "<rect class=\"ground\" x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\"/>\n"
    ));
    // The spine, drawn first so every node sits on top of it.
    if rail_to > rail_from {
        out.push_str(&format!(
            "<path class=\"rail\" d=\"M{} {rail_from}V{rail_to}\"/>\n",
            rail_x()
        ));
    }
    out.push_str(&body);
    out.push_str("</svg>\n");
    out
}

const fn rail_x() -> u32 {
    RAIL_X
}

/// A step's words, broken into lines no wider than the label column.
///
/// **The width driver turned out to be the words, not the columns.** `add where
/// name starts "q"` is written back out as one assignment per matching column,
/// so on a twelve-column table it is 262 characters and a picture 2,358 pixels
/// wide. Breaking it is the same answer wrapping gave the chips: show all of it,
/// and stop the page growing.
///
/// Broken at a space where there is one, because a break inside `total([x])`
/// reads as two different calls.
fn fold(label: &str, cap: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in label.split(' ') {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > cap {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
        // A single word longer than the column still has to break somewhere.
        while line.chars().count() > cap {
            let cut: String = line.chars().take(cap).collect();
            line = line.chars().skip(cap).collect();
            lines.push(cut);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn text_w(s: &str, ch: u32) -> u32 {
    super::scene::cells(s) as u32 * ch
}

struct Draw {
    out: String,
    width: u32,
    height: u32,
}

impl Draw {
    fn wide(&mut self, x: u32) {
        self.width = self.width.max(x);
        let _ = self.height;
    }

    /// A dot on the spine, one per step.
    fn node(&mut self, body: &mut String, x: u32, y: u32) {
        body.push_str(&format!("<circle class=\"node\" cx=\"{x}\" cy=\"{y}\" r=\"5\"/>\n"));
    }

    /// The mark where a sentence stopped checking.
    fn cross(&mut self, body: &mut String, x: u32, y: u32) {
        body.push_str(&format!(
            "<path class=\"cross\" d=\"M{} {}L{} {}M{} {}L{} {}\"/>\n",
            x - 5, y - 5, x + 5, y + 5, x + 5, y - 5, x - 5, y + 5
        ));
    }

    /// The step's words, folded to the label column and centered on the band.
    fn label(&mut self, body: &mut String, x: u32, mid: u32, text: &str, ink: Ink, size: u32) {
        let lines = fold(text, LABEL_CHARS);
        let ch = if size == FONT { CH } else { SMALL_CH };
        let block = lines.len() as u32 * LINE_H;
        let top = mid.saturating_sub(block / 2);
        for (i, line) in lines.iter().enumerate() {
            let w = text_w(line, ch);
            body.push_str(&format!(
                "<text class=\"ink-{}\" x=\"{x}\" y=\"{}\" textLength=\"{w}\" \
                 lengthAdjust=\"spacingAndGlyphs\">{}</text>\n",
                ink.class(),
                top + i as u32 * LINE_H + LINE_H / 2 + size / 3,
                escape(line)
            ));
            self.wide(x + w);
        }
    }

    fn note(&mut self, body: &mut String, x: u32, baseline: u32, text: &str, ink: Ink) {
        let w = text_w(text, SMALL_CH);
        body.push_str(&format!(
            "<text class=\"ink-{} small\" x=\"{x}\" y=\"{baseline}\" textLength=\"{w}\" \
             lengthAdjust=\"spacingAndGlyphs\">{}</text>\n",
            ink.class(),
            escape(text)
        ));
        self.wide(x + w);
    }

    /// **The table itself, and its width is the table's width.** This is the one
    /// thing the ladder could not say: a `summarize` that takes five columns
    /// away draws a bar a third the length of the one above it, and a reader
    /// sees that before reading anything.
    fn bar(&mut self, body: &mut String, x: u32, top: u32, chips: &[Chip], y: &mut u32) {
        // **Rows of `WRAP`, not one long line.** A forty-column table drawn on
        // one row is a picture nobody can read and a page nobody can print.
        // Wrapping keeps every column on the page and stops the width growing
        // with the table.
        let rows: Vec<&[Chip]> = if chips.is_empty() {
            vec![&[]]
        } else {
            chips.chunks(WRAP).collect()
        };
        let widest = rows
            .iter()
            .map(|r| {
                let mut w = 0;
                for (i, chip) in r.iter().enumerate() {
                    if i > 0 {
                        w += CHIP_GAP;
                    }
                    w += chip_w(chip);
                }
                w
            })
            .max()
            .unwrap_or(0);
        let w = widest + BAR_PAD * 2;
        let h = BAR_PAD * 2 + rows.len() as u32 * CHIP_H + (rows.len() as u32 - 1) * CHIP_GAP;
        body.push_str(&format!(
            "<rect class=\"bar\" x=\"{x}\" y=\"{top}\" width=\"{w}\" height=\"{h}\" rx=\"9\"/>\n"
        ));

        for (line, row) in rows.iter().enumerate() {
            let row_top = top + BAR_PAD + line as u32 * (CHIP_H + CHIP_GAP);
            let mut at = x + BAR_PAD;
            for chip in row.iter() {
                let cw = chip_w(chip);
                body.push_str(&format!(
                    "<rect class=\"chip chip-{}\" x=\"{at}\" y=\"{row_top}\" width=\"{cw}\" height=\"{CHIP_H}\" rx=\"6\"/>\n",
                    chip.ink.class()
                ));
                let tw = text_w(&chip.text, SMALL_CH);
                let baseline = row_top + CHIP_H / 2 + SMALL / 3;
                body.push_str(&format!(
                    "<text class=\"on-chip ink-{}\" x=\"{}\" y=\"{baseline}\" textLength=\"{tw}\" \
                     lengthAdjust=\"spacingAndGlyphs\">{}</text>\n",
                    chip.ink.class(),
                    at + CHIP_PAD,
                    escape(&chip.text)
                ));
                if let Some(kind) = &chip.kind {
                    let kw = text_w(kind, SMALL_CH);
                    body.push_str(&format!(
                        "<text class=\"ink-kind on-chip\" x=\"{}\" y=\"{baseline}\" textLength=\"{kw}\" \
                         lengthAdjust=\"spacingAndGlyphs\">{}</text>\n",
                        at + CHIP_PAD + tw,
                        escape(kind)
                    ));
                }
                at += cw + CHIP_GAP;
            }
        }
        self.wide(x + w);
        *y = top + h;
    }

    /// The columns this step took away, struck through and outside the bar,
    /// because they are no longer part of the table.
    fn gone(&mut self, body: &mut String, x: u32, y: &mut u32, chips: &[Chip]) {
        // **Wrapped on the same rule as the bar above it.** A step that takes
        // thirty columns away should not report that in a different shape from
        // the one it reports what it kept in.
        for row in chips.chunks(WRAP) {
            let mut at = x;
            for chip in row {
                let w = text_w(&chip.text, SMALL_CH);
                body.push_str(&format!(
                    "<text class=\"ink-dropped small\" x=\"{at}\" y=\"{}\" textLength=\"{w}\" \
                     lengthAdjust=\"spacingAndGlyphs\">{}</text>\n",
                    *y + SMALL,
                    escape(&chip.text)
                ));
                at += w + 12;
            }
            self.wide(at);
            *y += NOTE_H;
        }
    }

    /// The arrow from an arriving table up into the step that reads it.
    fn arrow(&mut self, body: &mut String, x: u32, from: u32, to: u32) {
        body.push_str(&format!(
            "<path class=\"flow\" d=\"M{x} {}V{}\"/>\n",
            from + BAR_H / 2,
            to + 6
        ));
        body.push_str(&format!(
            "<path class=\"head\" d=\"M{} {}L{x} {to}L{} {}Z\"/>\n",
            x - 4,
            to + 7,
            x + 4,
            to + 7
        ));
    }
}

fn chip_w(chip: &Chip) -> u32 {
    let kind = chip.kind.as_deref().map(|k| text_w(k, SMALL_CH)).unwrap_or(0);
    text_w(&chip.text, SMALL_CH) + kind + CHIP_PAD * 2
}

/// **Every selector is scoped to a picture**, for the reason the ladder's own
/// stylesheet is: set inline in a page this block joins that page's stylesheet,
/// and a bare `.table` or `.bar` would reach the page's own.
fn style() -> String {
    format!(
        "<style>\
         svg{{max-width:100%;height:auto}}\
         svg text{{font-family:ui-monospace,SFMono-Regular,'SF Mono',Menlo,Consolas,'DejaVu Sans Mono',monospace;font-size:{FONT}px}}\
         svg .small{{font-size:{SMALL}px}}\
         svg .on-chip{{font-size:{SMALL}px}}\
         svg .ground{{fill:#fbfaf8}}\
         svg .bar{{fill:#f0eee9;stroke:#e2ded6;stroke-width:1}}\
         svg .rail{{stroke:#cbc7bf;stroke-width:2;fill:none}}\
         svg .node{{fill:#fbfaf8;stroke:#a8a49b;stroke-width:2}}\
         svg .cross{{stroke:#a8514a;stroke-width:2;fill:none;stroke-linecap:round}}\
         svg .flow{{stroke:#a8a49b;stroke-width:2;fill:none}}\
         svg .head{{fill:#a8a49b}}\
         svg .chip{{fill:#ffffff;stroke:#ddd8ce;stroke-width:1}}\
         svg .chip-added{{fill:#e3f3e9;stroke:#8fc9a9}}\
         svg .chip-key{{fill:#e4ecfa;stroke:#9ab6e4}}\
         svg .chip-note{{fill:#efece7;stroke:#ddd8ce}}\
         svg .ink-source{{fill:#1a1a18;font-weight:600}}\
         svg .ink-step{{fill:#26251f}}\
         svg .ink-table{{fill:#1a1a18;font-weight:600}}\
         svg .ink-column{{fill:#57544c}}\
         svg .ink-kind{{fill:#a5a096}}\
         svg .ink-added{{fill:#14663d;font-weight:600}}\
         svg .ink-key{{fill:#27528f;font-weight:600}}\
         svg .ink-dropped{{fill:#a8514a;text-decoration:line-through}}\
         svg .ink-note{{fill:#8b867e}}\
         svg .ink-warn{{fill:#8a6410}}\
         svg .ink-caret{{fill:#a8514a;font-weight:600}}\
         @media (prefers-color-scheme:dark){{\
         svg .ground{{fill:#17181b}}\
         svg .bar{{fill:#202227;stroke:#2f3238}}\
         svg .rail{{stroke:#3d4046}}\
         svg .node{{fill:#17181b;stroke:#6b7078}}\
         svg .cross{{stroke:#e2908a}}\
         svg .flow{{stroke:#6b7078}}\
         svg .head{{fill:#6b7078}}\
         svg .chip{{fill:#26282d;stroke:#3a3d43}}\
         svg .chip-added{{fill:#1c3a2b;stroke:#3f7a58}}\
         svg .chip-key{{fill:#1e2b40;stroke:#3f5f8f}}\
         svg .chip-note{{fill:#232529;stroke:#3a3d43}}\
         svg .ink-source{{fill:#eceae5}}\
         svg .ink-step{{fill:#dedbd4}}\
         svg .ink-table{{fill:#eceae5}}\
         svg .ink-column{{fill:#b3afa6}}\
         svg .ink-kind{{fill:#6e6a63}}\
         svg .ink-added{{fill:#6fd39c}}\
         svg .ink-key{{fill:#8fb6f5}}\
         svg .ink-dropped{{fill:#e2908a}}\
         svg .ink-note{{fill:#7e7a73}}\
         svg .ink-warn{{fill:#d7a54c}}\
         svg .ink-caret{{fill:#e2908a}}\
         }}</style>\n"
    )
}

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

/// The schema a bar is drawn from, for a caller that has one rather than a
/// reading. Kept beside the emitter so nothing outside it builds chips.
#[allow(dead_code)]
fn from_schema(s: &Schema) -> Vec<Chip> {
    chips_of(s)
}
