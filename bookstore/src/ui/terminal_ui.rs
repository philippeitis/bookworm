use std::time::Duration;

use crossterm::event::{poll, read};

use tui::backend::Backend;
use tui::layout::Rect;
use tui::Terminal;

use crate::app::settings::{InterfaceStyle, NavigationSettings, Settings};
use crate::app::user_input::{CommandString, EditState};
use crate::app::{App, ApplicationError};
use crate::database::{AppDatabase, IndexableDatabase};
use crate::ui::views::{AppView, ApplicationTask, ColumnWidget, EditWidget, View};
use crate::ui::widgets::{BorderWidget, Widget};

#[derive(Default)]
pub(crate) struct UIState {
    pub(crate) style: InterfaceStyle,
    pub(crate) nav_settings: NavigationSettings,
    pub(crate) curr_command: CommandString,
    pub(crate) selected_column: usize,
}

pub(crate) struct AppInterface<D: AppDatabase, B> {
    app: App<D>,
    border_widget: BorderWidget,
    active_view: Box<dyn View<D, B>>,
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
                            AppView::ColumnView => {
                                self.active_view = Box::new(ColumnWidget { state })
                            }
                            AppView::EditView => {
                                if let Some(x) = self.app.selected() {
                                    self.active_view = Box::new(EditWidget {
                                        edit: EditState::new(&self.app.get_value(0, x), x),
                                        state,
                                    })
                                }
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

                    self.active_view.render_into_frame(&mut self.app, f, chunk);
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
