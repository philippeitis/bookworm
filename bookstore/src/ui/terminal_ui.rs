use std::time::Duration;

use crossterm::event::{poll, read};

use tui::backend::Backend;
use tui::layout::Rect;
use tui::Terminal;

use crate::app::app::ColumnUpdate;
use crate::app::settings::{InterfaceStyle, NavigationSettings, Settings};
use crate::app::user_input::{CommandString, EditState};
use crate::app::{App, ApplicationError};
use crate::database::IndexableDatabase;
use crate::ui::scrollable_text::{BlindOffset, ScrollableText};
use crate::ui::views::{
    AppView, ApplicationTask, ColumnWidget, EditWidget, HelpWidget, InputHandler, ResizableWidget,
};
use crate::ui::widgets::{BorderWidget, Widget};

#[derive(Default)]
pub(crate) struct UIState {
    pub(crate) style: InterfaceStyle,
    pub(crate) nav_settings: NavigationSettings,
    pub(crate) curr_command: CommandString,
    pub(crate) selected_column: usize,
}

trait ViewHandler<D: IndexableDatabase, B: Backend>: ResizableWidget<D, B> + InputHandler<D> {}

impl<D: IndexableDatabase, B: Backend> ViewHandler<D, B> for ColumnWidget {}
impl<D: IndexableDatabase, B: Backend> ViewHandler<D, B> for EditWidget {}
impl<D: IndexableDatabase, B: Backend> ViewHandler<D, B> for HelpWidget {}

pub(crate) struct AppInterface<D: IndexableDatabase, B: Backend> {
    app: App<D>,
    border_widget: BorderWidget,
    active_view: Box<dyn ViewHandler<D, B>>,
}

impl<D: IndexableDatabase, B: Backend> AppInterface<D, B> {
    /// Returns a new database, instantiated with the provided settings and database.
    ///
    /// # Arguments
    ///
    /// * ` name ` - The application instance name. Not to confused with the file name.
    /// * ` settings` - The application settings.
    /// * ` db ` - The database which contains books to be read.
    ///
    /// # Errors
    /// None.
    pub(crate) fn new<S: AsRef<str>>(
        name: S,
        settings: Settings,
        mut app: App<D>,
    ) -> Result<Self, ApplicationError> {
        app.set_selected_columns(settings.columns);

        Ok(AppInterface {
            app,
            border_widget: BorderWidget::new(name.as_ref().to_string()),
            active_view: Box::new(ColumnWidget {
                state: UIState {
                    style: settings.interface_style,
                    nav_settings: settings.navigation_settings,
                    curr_command: CommandString::new(),
                    selected_column: 0,
                },
                had_selected: false,
                offset: BlindOffset::new(),
                book_area: Default::default(),
            }),
        })
    }

    /// Reads and handles user input. On success, returns a bool
    /// indicating whether to continue or not.
    ///
    /// # Arguments
    ///
    /// * ` terminal ` - The current terminal.
    ///
    /// # Errors
    /// This function may error if executing a particular action fails.
    fn get_input(&mut self) -> Result<bool, ApplicationError> {
        loop {
            if poll(Duration::from_millis(500))? {
                match self.active_view.handle_input(read()?, &mut self.app)? {
                    ApplicationTask::Quit => return Ok(true),
                    ApplicationTask::Update => self.app.register_update(),
                    ApplicationTask::SwitchView(view) => {
                        self.app.register_update();
                        let state = self.active_view.take_state();
                        match view {
                            AppView::Columns => {
                                self.active_view = Box::new(ColumnWidget {
                                    state,
                                    had_selected: false,
                                    offset: BlindOffset::new(),
                                    book_area: Default::default(),
                                })
                            }
                            AppView::Edit => {
                                if let Some(x) = self.app.selected() {
                                    self.active_view = Box::new(EditWidget {
                                        edit: EditState::new(&self.app.get_value(0, x), x),
                                        state,
                                    })
                                }
                            }
                            AppView::Help => {
                                let help_string = self.app.take_help_string();
                                self.active_view = Box::new(HelpWidget {
                                    state,
                                    text: ScrollableText::new(help_string),
                                })
                            }
                        }
                    }
                    ApplicationTask::DoNothing => {}
                }
                break;
            }
        }
        Ok(false)
    }

    /// Runs the application - including handling user inputs and refreshing the output.
    ///
    /// # Arguments
    ///
    /// * ` terminal ` - The terminal to output text to.
    ///
    /// # Errors
    /// This function will return an error if running the program fails for any reason.
    pub(crate) fn run(&mut self, terminal: &mut Terminal<B>) -> Result<(), ApplicationError> {
        loop {
            self.app.apply_sort()?;
            self.app.update_column_data()?;

            if self.app.take_update() {
                terminal.draw(|f| {
                    self.border_widget.saved = self.app.saved();
                    self.border_widget.render_into_frame(f, f.size());

                    let chunk = {
                        let s = f.size();
                        Rect::new(
                            s.x + 1,
                            s.y + 1,
                            s.width.saturating_sub(2),
                            s.height.saturating_sub(2),
                        )
                    };

                    if let Some(chunk_size) = self.active_view.allocate_chunk(chunk) {
                        if self.app.refresh_window_size(chunk_size) {
                            self.app.set_column_update(ColumnUpdate::Regenerate);
                            let _ = self.app.update_column_data();
                        }
                    }

                    self.active_view.render_into_frame(&self.app, f, chunk);
                })?;
            }

            match self.get_input() {
                Ok(true) => return Ok(terminal.clear()?),
                _ => {
                    // TODO: Handle errors here.
                }
            }
        }
    }
}
