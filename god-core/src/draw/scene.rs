//! What a drawing is, before anything decides how big a character is.
//!
//! **Everything here is measured in cells of a fixed grid — never in pixels.**
//! A cell is one character wide and one line tall, and a position is two whole
//! numbers. That is the whole discipline, and three things fall out of it.
//!
//! The layout is written once. A scene can be printed as text or drawn as a
//! picture, and neither is a second layout that has to be kept in step with the
//! first: the picture multiplies these coordinates by a character's width and
//! does nothing else.
//!
//! The printed version is a proof of the drawn one. If the ladder reads
//! correctly in a terminal then the positions are right, because they are the
//! same positions.
//!
//! And the output is the same every time. Every coordinate here is a whole
//! number worked out from the length of a string, so there is no rounding for
//! two runs to disagree about, and nothing in this module iterates a container
//! whose order is not written down.

/// What a piece of text in the drawing *is*, which is not the same as how it
/// looks. An emitter chooses the glyph, the weight and the color; this says
/// what the reader is being told.
///
/// **The list is closed on purpose.** It is small enough to hold in the head,
/// and a drawing that needs a fourteenth kind of ink is usually a drawing that
/// is trying to say too much.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// The spine down the left, and the elbows that hang off it.
    Rail,
    /// The table the pipeline starts from.
    Source,
    /// One step, in the words the person wrote.
    Step,
    /// Another table, arriving partway down.
    Table,
    /// A column that was here before this step and is here after it.
    Column,
    /// A column this step makes.
    Added,
    /// A column this step takes away.
    Dropped,
    /// A column two tables matched on, which arrives once rather than twice.
    Key,
    /// What a column holds.
    Kind,
    /// Something the grammar settled, or what became of the rows.
    Note,
    /// Something the grammar cannot promise.
    Warn,
    /// The mark under the words that were refused.
    Caret,
}

impl Ink {
    /// Every kind, so a check can walk them rather than restate them.
    ///
    /// **An emitter that has never been told how to draw one of these should
    /// fail loudly rather than draw it as ordinary text.** A list written down
    /// in the emitter would agree with itself and with nothing else; this is the
    /// list, and the test that walks it fails the day a kind is added and left
    /// unstyled.
    pub const ALL: &'static [Ink] = &[
        Ink::Rail,
        Ink::Source,
        Ink::Step,
        Ink::Table,
        Ink::Column,
        Ink::Added,
        Ink::Dropped,
        Ink::Key,
        Ink::Kind,
        Ink::Note,
        Ink::Warn,
        Ink::Caret,
    ];

    /// The class an emitter styles this with. One word, and the same word in the
    /// stylesheet, so the two cannot be spelled differently.
    pub fn class(self) -> &'static str {
        match self {
            Ink::Rail => "rail",
            Ink::Source => "source",
            Ink::Step => "step",
            Ink::Table => "table",
            Ink::Column => "column",
            Ink::Added => "added",
            Ink::Dropped => "dropped",
            Ink::Key => "key",
            Ink::Kind => "kind",
            Ink::Note => "note",
            Ink::Warn => "warn",
            Ink::Caret => "caret",
        }
    }
}

/// One line of the drawing.
#[derive(Debug, Clone)]
pub struct Row {
    /// Which step this line belongs to. Lines sharing a band are drawn
    /// together, which is what lets a picture shade one step at a time without
    /// the scene having to know that is why it was asked.
    pub band: u16,
    pub cells: Vec<Cell>,
}

/// A run of text at a position, and what it means.
#[derive(Debug, Clone)]
pub struct Cell {
    pub col: u16,
    pub text: String,
    pub ink: Ink,
}

/// A whole drawing: lines of cells, and how wide the widest one is.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub rows: Vec<Row>,
    pub width: u16,
}

impl Scene {
    pub fn new() -> Self {
        Scene::default()
    }

    pub fn push(&mut self, row: Row) {
        let end = row.cells.last().map(|c| c.col + cells(&c.text)).unwrap_or(0);
        self.width = self.width.max(end);
        self.rows.push(row);
    }

    /// A line with nothing on it, which is how bands are kept apart.
    pub fn blank(&mut self, band: u16) {
        self.rows.push(Row { band, cells: Vec::new() });
    }
}

/// Builds one line left to right, keeping the column count for you.
///
/// Doing that by hand is where a grid layout goes wrong: a `col` worked out
/// from `text.len()` is a count of *bytes*, and a column called `지역` would
/// push everything after it out of true. Everything measured here goes through
/// [`cells`].
pub struct RowBuilder {
    band: u16,
    cells: Vec<Cell>,
    col: u16,
}

impl RowBuilder {
    pub fn new(band: u16) -> Self {
        RowBuilder { band, cells: Vec::new(), col: 0 }
    }

    /// Move to a column. Never backwards: a caller asking for a column already
    /// passed gets the current one, because text that overlaps text is a defect
    /// the drawing cannot show and a reader cannot see.
    pub fn at(mut self, col: u16) -> Self {
        self.col = self.col.max(col);
        self
    }

    pub fn gap(mut self, n: u16) -> Self {
        self.col += n;
        self
    }

    pub fn put(mut self, text: impl Into<String>, ink: Ink) -> Self {
        let text = text.into();
        if text.is_empty() {
            return self;
        }
        let width = cells(&text);
        self.cells.push(Cell { col: self.col, text, ink });
        self.col += width;
        self
    }

    /// The column the next thing would start at.
    pub fn end(&self) -> u16 {
        self.col
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn done(self) -> Row {
        Row { band: self.band, cells: self.cells }
    }
}

/// How many cells a string takes up.
///
/// **Not `len`, which counts bytes, and not `chars().count()` alone.** A wide
/// character occupies two cells in every terminal that draws one, so a Korean
/// or Japanese column name would misalign every strip to its right if it were
/// counted as one.
pub fn cells(text: &str) -> u16 {
    text.chars().map(char_cells).sum()
}

/// **This is a table rather than a dependency, and it is deliberately partial.**
/// It covers East Asian Wide and Fullwidth, which is where real column names
/// live; the ambiguous-width characters of UAX #11 are counted as one, which is
/// what a terminal outside East Asia does with them. Combining marks are
/// counted as one rather than nothing, which is wrong and has never come up in
/// a column name.
fn char_cells(c: char) -> u16 {
    match c as u32 {
        0x1100..=0x115F        // Hangul Jamo, the leading consonants
        | 0x2E80..=0x303E      // CJK radicals through the CJK symbols
        | 0x3041..=0x33FF      // kana, Bopomofo, Hangul compatibility, CJK compat
        | 0x3400..=0x4DBF      // CJK extension A
        | 0x4E00..=0x9FFF      // CJK unified ideographs
        | 0xA000..=0xA4CF      // Yi
        | 0xAC00..=0xD7A3      // Hangul syllables
        | 0xF900..=0xFAFF      // CJK compatibility ideographs
        | 0xFE10..=0xFE19
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60      // fullwidth forms
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F    // the pictographs, where a terminal draws two
        | 0x1F900..=0x1F9FF => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_name_takes_two_cells() {
        assert_eq!(cells("region"), 6);
        assert_eq!(cells("지역"), 4);
        // The case the drawing would get wrong if it counted bytes: this is
        // six bytes and two cells, and a strip placed at column 6 after it
        // would sit four columns too far right.
        assert_eq!("지역".len(), 6);
    }

    #[test]
    fn a_builder_never_goes_backwards() {
        let row = RowBuilder::new(0).at(4).put("here", Ink::Step).at(2).put("!", Ink::Step).done();
        assert_eq!(row.cells[0].col, 4);
        assert_eq!(row.cells[1].col, 8);
    }

    #[test]
    fn width_is_the_widest_line() {
        let mut scene = Scene::new();
        scene.push(RowBuilder::new(0).put("ab", Ink::Step).done());
        scene.push(RowBuilder::new(1).at(10).put("cd", Ink::Step).done());
        assert_eq!(scene.width, 12);
    }
}
