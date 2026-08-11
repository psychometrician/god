//! A scene printed as a ladder, for a terminal.
//!
//! **This emitter does one thing: it puts cells where the scene says.** It makes
//! no layout decisions, because there are none left to make by the time a scene
//! exists — which is the point of building one.
//!
//! Ink is dropped here rather than shown. A terminal that can color would look
//! better and would also make the output depend on whether it is a terminal, and
//! the drawing this project wants is the one that is the same in a pipe, in a
//! golden file, and on a page.

use super::scene::{cells, Scene};

/// The scene as lines of text, each ending in a newline.
///
/// Trailing spaces are trimmed. They are invisible on screen and they are the
/// difference between two golden files that a reader would call identical.
pub fn render(scene: &Scene) -> String {
    let mut out = String::new();
    for row in &scene.rows {
        let mut line = String::new();
        let mut col = 0u16;
        for cell in &row.cells {
            while col < cell.col {
                line.push(' ');
                col += 1;
            }
            line.push_str(&cell.text);
            col += cells(&cell.text);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}
