//! Clickable and scrollable screen regions, recorded while drawing.

use ratatui::layout::{Position, Rect};

use crate::config::ViewType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    SectionTab(usize),
    PrevSectionArrow,
    NextSectionArrow,
    NewSection,
    TabsRow,
    SearchBar,
    List,
    Row(usize),
    Preview,
    PreviewTab(usize),
    View(ViewType),
    Help,
    HelpPopup,
    Backdrop,
    Suggestion(usize),
    ConfirmPopup,
    ConfirmButton(crate::confirm::Choice),
}

impl Target {
    fn scrollable(self) -> bool {
        matches!(
            self,
            Target::TabsRow | Target::List | Target::Preview | Target::HelpPopup | Target::Backdrop
        )
    }
}

/// Regions in draw order; a region drawn later covers the ones below it.
#[derive(Default, Debug)]
pub struct HitMap {
    regions: Vec<(Rect, Target)>,
}

impl HitMap {
    pub fn push(&mut self, area: Rect, target: Target) {
        if !area.is_empty() {
            self.regions.push((area, target));
        }
    }

    pub fn at(&self, x: u16, y: u16) -> Option<Target> {
        self.topmost(x, y, |_| true)
    }

    pub fn scrollable_at(&self, x: u16, y: u16) -> Option<Target> {
        self.topmost(x, y, Target::scrollable)
    }

    fn topmost(&self, x: u16, y: u16, keep: impl Fn(Target) -> bool) -> Option<Target> {
        self.regions
            .iter()
            .rev()
            .find(|(r, t)| r.contains(Position { x, y }) && keep(*t))
            .map(|(_, t)| *t)
    }
}
