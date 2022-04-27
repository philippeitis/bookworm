use tui::layout::{Constraint, Direction, Layout, Rect};

pub trait RectExt {
    fn contains(&self, point: &(u16, u16)) -> bool;
}

impl RectExt for Rect {
    fn contains(&self, point: &(u16, u16)) -> bool {
        point.0 >= self.x
            && point.0 < self.x + self.width
            && point.1 >= self.y
            && point.1 < self.y + self.height
    }
}

pub trait LayoutGenerator {
    fn layout(&self, chunk: Rect) -> Vec<Rect>;
}

pub struct EditLayout {}

impl LayoutGenerator for EditLayout {
    fn layout(&self, chunk: Rect) -> Vec<Rect> {
        let mut layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(chunk.height.saturating_sub(1)),
                Constraint::Length(1),
            ])
            .split(chunk);
        layout.swap(0, 1);
        layout
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
            .split(hchunks[0]);

        vchunks.swap(0, 1);
        vchunks.push(hchunks[1]);
        vchunks
    }
}

impl<F> LayoutGenerator for F
where
    F: Fn(Rect) -> Vec<Rect>,
{
    fn layout(&self, chunk: Rect) -> Vec<Rect> {
        self(chunk)
    }
}
