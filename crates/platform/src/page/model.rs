//! What a native page shows: a description the browser builds (title, left navigation,
//! sections of cards with rows and controls). The page window only draws it and reports actions.

/// A whole page, like Chrome's settings: navigation on the left, cards on the right.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageDoc {
    pub title: String,
    /// Placeholder of the search box at the top ("" = no search box).
    pub search: String,
    pub nav: Vec<NavItem>,
    pub sections: Vec<Section>,
    /// Short message shown at the bottom (after an action), "" = none.
    pub toast: String,
    /// Each section is a page of its own, picked in the navigation (Settings). Otherwise all
    /// sections show and the search box keeps only the matching rows (History, Bookmarks).
    pub paged: bool,
    /// Navigation entry drawn selected on a page that is not paged.
    pub nav_on: String,
    /// The browser runs the search (`page\tsearch\t\t<text>`) and sends the results as a new
    /// description, instead of the page filtering what it has.
    pub remote_search: bool,
    /// A start page (New Tab, Incognito): centred logo or `hero_icon`, the title, `hero_text`,
    /// a search box with the `fakebox` placeholder ("" = none) — a real text field (id
    /// `fakebox`: each change is `page\tinput\tfakebox\t<text>`, Enter `page\ttext\tfakebox\t<text>`,
    /// ↑ ↓ Escape and leaving it `page\tkey\tfakebox\t<up|down|escape|blur>`) —, tiles (section
    /// `start::TILES`), cards, and the navigation as chips.
    pub start: bool,
    pub hero_icon: String,
    pub hero_text: String,
    pub fakebox: String,
    /// The start page's logo bounces: dropped in once, or on and on (while the first web page
    /// gets ready).
    pub logo_bounce: Bounce,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Bounce {
    #[default]
    None,
    Once,
    Loop,
}

/// One entry of the left navigation; clicking it scrolls to the section with the same id,
/// or reports `id` as an action when no section has it (a link to another page).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NavItem {
    pub id: String,
    pub label: String,
    /// Icon-font glyph.
    pub icon: String,
}

/// A titled block of the page: one or more cards.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Section {
    pub id: String,
    pub title: String,
    /// Extra words the search box matches (besides the titles of the section and its rows).
    pub keywords: String,
    pub cards: Vec<Vec<Row>>,
}

/// One line of a card.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub title: String,
    pub sub: String,
    pub control: Control,
    /// Indented under the row above (a dependent setting).
    pub indent: bool,
}

/// The control at the right of a row (or the row's own behaviour).
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Control {
    #[default]
    None,
    /// Small caps heading inside a card.
    Heading,
    Toggle { id: String, on: bool },
    /// A dropdown: (value, label) options; opens a native menu.
    Select { id: String, value: String, options: Vec<(String, String)> },
    /// One choice of a group: the action reports `value`.
    Radio { id: String, value: String, on: bool },
    Buttons(Vec<Button>),
    /// A text field with a button that reports its text (or Enter in the field). Without a
    /// button (`""`) it is part of a form: every change is reported as `input`.
    Text { id: String, value: String, placeholder: String, button: String },
    /// The whole row is clickable (opens something), drawn with a chevron.
    Link { id: String },
    /// An avatar with an initial at the start of the row (accounts).
    Avatar { initial: String },
    /// A list entry (a visited page, a bookmark): an icon at the start, the whole row opens `id`
    /// (reported as a link), and a remove button reports `remove`.
    Entry { id: String, icon: String, remove: String },
    /// A figure shown at the right (memory of a process, a total).
    Value(String),
    /// An illustration (built around an icon-font `glyph`) with the row's title and text centred
    /// under it: empty states ("No history yet").
    Illustration { glyph: String },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Button {
    pub id: String,
    pub label: String,
    pub style: ButtonStyle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonStyle {
    #[default]
    Normal,
    Primary,
    Danger,
}

impl Row {
    pub fn new(title: &str, sub: &str, control: Control) -> Row {
        Row { title: title.into(), sub: sub.into(), control, indent: false }
    }

    pub fn heading(text: &str) -> Row {
        Row { title: text.into(), control: Control::Heading, ..Default::default() }
    }

    pub fn indented(mut self) -> Row {
        self.indent = true;
        self
    }
}

impl Button {
    pub fn new(id: &str, label: &str, style: ButtonStyle) -> Button {
        Button { id: id.into(), label: label.into(), style }
    }
}

impl Row {
    /// Does the search text match the row's texts?
    pub fn matches(&self, needle: &str) -> bool {
        needle.is_empty() || format!("{} {}", self.title, self.sub).to_lowercase().contains(&needle.to_lowercase())
    }
}

impl Section {
    /// Does the search text match this section (titles, row texts or keywords)?
    pub fn matches(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let rows = self.cards.iter().flatten().map(|r| format!("{} {}", r.title, r.sub)).collect::<Vec<_>>().join(" ");
        format!("{} {} {}", self.title, self.keywords, rows).to_lowercase().contains(&needle.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_titles_rows_and_keywords() {
        let s = Section {
            id: "privacy".into(),
            title: "Privacy and security".into(),
            keywords: "cookies".into(),
            cards: vec![vec![Row::new("Safe Browsing", "SmartScreen", Control::None)]],
        };
        assert!(s.matches(""));
        assert!(s.matches("COOKIES"));
        assert!(s.matches("smartscreen"));
        assert!(!s.matches("downloads"));
    }
}
