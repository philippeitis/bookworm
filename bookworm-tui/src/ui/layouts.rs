use tui::layout::{Constraint, Direction, Layout, Rect};

pub trait RectExt {
    fn contains(&self, point: &(u16, u16)) -> bool;
}

impl RectExt for Rect {
    fn contains(&self, point: &(u16, u16)) -> bool {
        point >= &(self.x, self.y) && point < &(self.x + self.width, self.y + self.height)
    }
}

pub trait LayoutGenerator {
    fn layout(&self, chunk: Rect) -> Vec<Rect>;
}

pub struct EditLayout {}

impl LayoutGenerator for EditLayout {
    fn layout(&self, chunk: Rect) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(chunk.height.saturating_sub(1)),
                Constraint::Length(1),
            ])
            .split(chunk)
    }
}

pub struct ColumnBookLayout {}

impl LayoutGenerator for ColumnBookLayout {
    fn layout(&self, chunk: Rect) -> Vec<Rect> {
        let hchunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
            .split(chunk);

        let mut vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(hchunks[0].height.saturating_sub(1)),
                Constraint::Length(1),
            ])
            .split(chunk);

        vchunks.push(hchunks[1]);
        vchunks
    }
}
