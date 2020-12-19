pub(crate) struct ScrollableText {
    text: String,
    height: usize,
    offset: usize,
    window_height: usize,
}

impl ScrollableText {
    pub(crate) fn new(text: impl AsRef<str>) -> Self {
        let text = text.as_ref().to_string();
        Self {
            height: text.lines().count(),
            text,
            offset: 0,
            window_height: 0,
        }
    }

    pub(crate) fn scroll_down(&mut self, scroll: usize) {
        self.offset = self
            .height
            .saturating_sub(self.window_height)
            .min(self.offset.saturating_add(scroll));
    }

    pub(crate) fn scroll_up(&mut self, scroll: usize) {
        self.offset = self.offset.saturating_sub(scroll);
    }

    pub(crate) fn page_up(&mut self) {
        self.scroll_up(self.window_height)
    }

    pub(crate) fn page_down(&mut self) {
        self.scroll_down(self.window_height)
    }

    pub(crate) fn home(&mut self) {
        self.offset = 0;
    }

    pub(crate) fn end(&mut self) {
        self.offset = self.height.saturating_sub(self.window_height);
    }

    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    pub(crate) fn refresh_window_height(&mut self, height: usize) {
        self.window_height = height;
    }

    pub(crate) fn text(&self) -> &String {
        &self.text
    }
}

pub(crate) struct BlindOffset {
    offset: usize,
    window_height: usize,
}

#[allow(dead_code)]
impl BlindOffset {
    pub(crate) fn new() -> Self {
        Self {
            offset: 0,
            window_height: 0,
        }
    }

    pub(crate) fn scroll_down(&mut self, scroll: usize) {
        self.offset = self.offset.saturating_add(scroll);
    }

    pub(crate) fn scroll_up(&mut self, scroll: usize) {
        self.offset = self.offset.saturating_sub(scroll);
    }

    pub(crate) fn page_up(&mut self) {
        self.scroll_up(self.window_height)
    }

    pub(crate) fn page_down(&mut self) {
        self.scroll_down(self.window_height)
    }

    pub(crate) fn home(&mut self) {
        self.offset = 0;
    }

    pub(crate) fn end(&mut self) {
        self.offset = usize::MAX;
    }

    pub(crate) fn offset_with_height(&mut self, height: usize) -> usize {
        self.offset = height.saturating_sub(self.window_height).min(self.offset);
        self.offset
    }

    pub(crate) fn refresh_window_height(&mut self, height: usize) {
        self.window_height = height;
    }
}
