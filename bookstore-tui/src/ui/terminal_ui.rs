use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;
use std::time::Duration;

use crossterm::event::{poll, read};
use crossterm::ErrorKind;

use tui::backend::Backend;
use tui::layout::Rect;
use tui::Terminal;

use bookstore_app::settings::{InterfaceStyle, NavigationSettings, Settings};
use bookstore_app::table_view::TableView;
use bookstore_app::user_input::{CommandString, EditState};
use bookstore_app::{App, ApplicationError};
use bookstore_database::{BookView, DatabaseError, IndexableDatabase, SearchableBookView};

use crate::ui::scrollable_text::{BlindOffset, ScrollableText};
use crate::ui::views::{
    AppView, ApplicationTask, ColumnWidget, EditWidget, HelpWidget, InputHandler, ResizableWidget,
};
use crate::ui::widgets::{BorderWidget, Widget};

#[derive(Debug)]
pub(crate) enum TuiError {
    Terminal(ErrorKind),
    Application(ApplicationError),
    Io(std::io::Error),
    Database(DatabaseError),
}

impl From<ErrorKind> for TuiError {
    fn from(e: ErrorKind) -> Self {
        TuiError::Terminal(e)
    }
}

impl From<ApplicationError> for TuiError {
    fn from(e: ApplicationError) -> Self {
        TuiError::Application(e)
    }
}

impl From<std::io::Error> for TuiError {
    fn from(e: std::io::Error) -> Self {
        TuiError::Io(e)
    }
}

impl From<DatabaseError> for TuiError {
    fn from(e: DatabaseError) -> Self {
        TuiError::Database(e)
    }
}

pub(crate) struct UIState<D: IndexableDatabase> {
    pub(crate) style: InterfaceStyle,
    pub(crate) nav_settings: NavigationSettings,
    pub(crate) curr_command: CommandString,
    pub(crate) selected_column: usize,
    pub(crate) table_view: TableView,
    pub(crate) book_view: SearchableBookView<D>,
}

impl<D: IndexableDatabase> UIState<D> {
    pub(crate) fn modify_bv(&mut self, f: impl Fn(&mut SearchableBookView<D>) -> bool) -> bool {
        f(&mut self.book_view)
    }

    pub(crate) fn update_column_data(&mut self) -> Result<(), ApplicationError> {
        self.table_view.regenerate_columns(&self.book_view)
    }

    pub(crate) fn get_selected_column_value(&self, index: usize) -> &str {
        &self.table_view.get_column(self.selected_column)[index]
    }

    pub(crate) fn num_cols(&self) -> usize {
        self.table_view.selected_cols().len()
    }
}

trait ViewHandler<D: IndexableDatabase, B: Backend>: ResizableWidget<D, B> + InputHandler<D> {}

impl<D: IndexableDatabase, B: Backend> ViewHandler<D, B> for ColumnWidget<D> {}
impl<D: IndexableDatabase, B: Backend> ViewHandler<D, B> for EditWidget<D> {}
impl<D: IndexableDatabase, B: Backend> ViewHandler<D, B> for HelpWidget<D> {}

pub(crate) struct AppInterface<'a, D: 'a + IndexableDatabase, B: Backend> {
    app: App<D>,
    border_widget: BorderWidget,
    active_view: Box<dyn ViewHandler<D, B> + 'a>,
    ui_state: Rc<RefCell<UIState<D>>>,
    ui_updated: bool,
}

impl<'a, D: 'a + IndexableDatabase, B: Backend> AppInterface<'a, D, B> {
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
        app: App<D>,
    ) -> Result<Self, TuiError> {
        let state = Rc::new(RefCell::new(UIState {
            style: settings.interface_style,
            nav_settings: settings.navigation_settings,
            curr_command: CommandString::new(),
            selected_column: 0,
            table_view: TableView::from(settings.columns),
            book_view: app.new_book_view(),
        }));
        Ok(AppInterface {
            border_widget: BorderWidget::new(name.as_ref().to_string()),
            active_view: Box::new(ColumnWidget {
                state: state.clone(),
                had_selected: false,
                offset: BlindOffset::new(),
                book_area: Rect::default(),
            }),
            ui_updated: false,
            ui_state: state,
            app,
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
    fn get_input(&mut self) -> Result<bool, TuiError> {
        loop {
            if poll(Duration::from_millis(500))? {
                match self.active_view.handle_input(read()?, &mut self.app)? {
                    ApplicationTask::Quit => return Ok(true),
                    ApplicationTask::SwitchView(view) => {
                        self.ui_updated = true;
                        match view {
                            AppView::Columns => {
                                self.active_view = Box::new(ColumnWidget {
                                    state: self.ui_state.clone(),
                                    had_selected: false,
                                    offset: BlindOffset::new(),
                                    book_area: Rect::default(),
                                })
                            }
                            AppView::Edit => {
                                let state = self.ui_state.deref().borrow();
                                if let Some(x) = state.book_view.selected() {
                                    self.active_view = Box::new(EditWidget {
                                        edit: EditState::new(state.get_selected_column_value(x), x),
                                        state: self.ui_state.clone(),
                                    });
                                }
                            }
                            AppView::Help => {
                                let help_string = self.app.take_help_string();
                                self.active_view = Box::new(HelpWidget {
                                    state: self.ui_state.clone(),
                                    text: ScrollableText::new(help_string),
                                })
                            }
                        }
                    }
                    ApplicationTask::UpdateUI => {
                        self.ui_updated = true;
                    }
                    ApplicationTask::DoNothing => {}
                }
                break;
            }
        }
        Ok(false)
    }

    fn take_update(&mut self) -> bool {
        std::mem::replace(&mut self.ui_updated, false)
    }

    /// Runs the application - including handling user inputs and refreshing the output.
    ///
    /// # Arguments
    ///
    /// * ` terminal ` - The terminal to output text to.
    ///
    /// # Errors
    /// This function will return an error if running the program fails for any reason.
    pub(crate) fn run(&mut self, terminal: &mut Terminal<B>) -> Result<(), TuiError> {
        loop {
            {
                let mut state = self.ui_state.borrow_mut();
                self.app.apply_sort(&mut state.book_view)?;
            }

            if self.app.take_update() || self.take_update() {
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
                        let mut state = self.ui_state.borrow_mut();
                        state.book_view.refresh_window_size(chunk_size);
                        let _ = state.update_column_data();
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
