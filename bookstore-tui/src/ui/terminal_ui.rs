use std::cell::RefCell;
use std::ops::Deref;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::ErrorKind;

use tui::backend::Backend;
use tui::layout::Rect;
use tui::Terminal;

use bookstore_app::settings::{
    DatabaseSettings, InterfaceSettings, InterfaceStyle, NavigationSettings, Settings,
};
use bookstore_app::table_view::TableView;
use bookstore_app::user_input::{CommandString, EditState};
use bookstore_app::{App, ApplicationError};
use bookstore_database::bookview::BookViewError;
use bookstore_database::{BookView, DatabaseError, IndexableDatabase, SearchableBookView};

use crate::ui::scrollable_text::ScrollableText;
use crate::ui::views::{
    AppView, ApplicationTask, ColumnWidget, EditWidget, HelpWidget, InputHandler, ResizableWidget,
};
use crate::ui::widgets::{BorderWidget, Widget};

use bookstore_database::paged_cursor::{RelativeSelection, Selection};

#[derive(Debug)]
pub(crate) enum TuiError<DBError> {
    Application(ApplicationError<DBError>),
    Database(DatabaseError<DBError>),
    Io(std::io::Error),
    Terminal(ErrorKind),
}

pub enum AppEvent {
    UserInput(Event),
}

impl<DBError> From<ApplicationError<DBError>> for TuiError<DBError> {
    fn from(e: ApplicationError<DBError>) -> Self {
        TuiError::Application(e)
    }
}

impl<DBError> From<DatabaseError<DBError>> for TuiError<DBError> {
    fn from(e: DatabaseError<DBError>) -> Self {
        TuiError::Database(e)
    }
}

impl<DBError> From<std::io::Error> for TuiError<DBError> {
    fn from(e: std::io::Error) -> Self {
        TuiError::Io(e)
    }
}

pub(crate) struct UIState<D: IndexableDatabase> {
    pub(crate) style: InterfaceStyle,
    pub(crate) nav_settings: NavigationSettings,
    pub(crate) curr_command: CommandString,
    pub(crate) selected_column: usize,
    pub(crate) table_view: TableView,
    pub(crate) book_view: SearchableBookView<D>,
    // pub(crate) command_log: Vec<CommandString>,
}

impl<D: IndexableDatabase> UIState<D> {
    pub(crate) fn modify_bv(&mut self, f: impl Fn(&mut SearchableBookView<D>) -> bool) -> bool {
        f(&mut self.book_view)
    }

    pub(crate) fn update_column_data(&mut self) -> Result<(), BookViewError<D::Error>> {
        self.table_view.regenerate_columns(&self.book_view)
    }

    pub(crate) fn selected_table_value(&self) -> Option<Vec<&str>> {
        let selected_column = self.table_view.get_column(self.selected_column);
        Some(
            self.book_view
                .relative_selections()?
                .iter()
                .map(|x| selected_column[x].as_str())
                .collect(),
        )
    }

    pub(crate) fn num_cols(&self) -> usize {
        self.table_view.selected_cols().len()
    }

    pub(crate) fn selected(&self) -> Option<(usize, RelativeSelection)> {
        Some((self.selected_column, self.book_view.relative_selections()?))
    }

    pub(crate) fn make_selection_visible(&mut self) -> Result<(), BookViewError<D::Error>> {
        if self.book_view.make_selection_visible() {
            self.table_view.regenerate_columns(&self.book_view)?;
        }
        Ok(())
    }
}

trait ViewHandler<D: IndexableDatabase, B: Backend>: ResizableWidget<D, B> + InputHandler<D> {}

impl<D: IndexableDatabase, B: Backend> ViewHandler<D, B> for ColumnWidget<D> {}
impl<D: IndexableDatabase, B: Backend> ViewHandler<D, B> for EditWidget<D> {}
impl<D: IndexableDatabase, B: Backend> ViewHandler<D, B> for HelpWidget<D> {}

// TODO: Use channels to allow CTRL+Q when application freezes
//          Also, allow text input / waiting animation
pub(crate) struct AppInterface<'a, D: 'a + IndexableDatabase, B: Backend> {
    app: App<D>,
    border_widget: BorderWidget,
    active_view: Box<dyn ViewHandler<D, B> + 'a>,
    ui_state: Rc<RefCell<UIState<D>>>,
    ui_updated: bool,
    settings_path: Option<PathBuf>,
    event_receiver: Receiver<AppEvent>,
    event_sender: Sender<AppEvent>,
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
    pub(crate) fn new<S: Into<String>>(
        name: S,
        settings: InterfaceSettings,
        settings_path: Option<PathBuf>,
        app: App<D>,
    ) -> Self {
        let state = Rc::new(RefCell::new(UIState {
            style: settings.interface_style,
            nav_settings: settings.navigation_settings,
            curr_command: CommandString::new(),
            selected_column: 0,
            table_view: TableView::from(settings.columns),
            book_view: app.new_book_view(),
        }));
        let (event_sender, event_receiver) = std::sync::mpsc::channel();
        AppInterface {
            border_widget: BorderWidget::new(name.into(), app.db_path()),
            active_view: Box::new(ColumnWidget {
                state: state.clone(),
                book_widget: None,
                command_widget_selected: false,
            }),
            ui_updated: false,
            ui_state: state,
            app,
            settings_path,
            event_receiver,
            event_sender,
        }
    }

    pub fn create_sender(&self) -> Sender<AppEvent> {
        self.event_sender.clone()
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
    fn get_input(&mut self) -> Result<bool, TuiError<D::Error>> {
        loop {
            if let Ok(event) = self.event_receiver.recv_timeout(Duration::from_millis(500)) {
                let event = match event {
                    AppEvent::UserInput(e) => e,
                };
                match event {
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('q'),
                        modifiers: KeyModifiers::CONTROL,
                    }) => return Ok(true),
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('s'),
                        modifiers: KeyModifiers::CONTROL,
                    }) => {
                        self.app.save()?;
                        return Ok(false);
                    }
                    _ => {}
                }
                match self.active_view.handle_input(event, &mut self.app)? {
                    ApplicationTask::Quit => return Ok(true),
                    ApplicationTask::SwitchView(view) => {
                        self.ui_updated = true;
                        match view {
                            AppView::Columns => {
                                self.active_view = Box::new(ColumnWidget {
                                    state: self.ui_state.clone(),
                                    book_widget: None,
                                    command_widget_selected: false,
                                })
                            }
                            AppView::Edit => {
                                {
                                    let _ =
                                        self.ui_state.deref().borrow_mut().make_selection_visible();
                                }

                                let state = self.ui_state.deref().borrow();
                                if let Some(selected_str) = state.selected_table_value() {
                                    self.active_view = Box::new(EditWidget {
                                        edit: EditState::new(selected_str[0]),
                                        focused: true,
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
    pub(crate) fn run(&mut self, terminal: &mut Terminal<B>) -> Result<(), TuiError<D::Error>> {
        loop {
            {
                let mut state = self.ui_state.borrow_mut();
                self.app.apply_sort(&mut state.book_view)?;
            }

            if self.app.take_update() | self.take_update() {
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

                    self.active_view.prepare_render(chunk);
                    self.active_view.render_into_frame(f, chunk);
                })?;
            }

            match self.get_input() {
                Ok(true) => {
                    self.write_settings()?;
                    return Ok(terminal.clear()?);
                }
                Ok(false) => {}
                Err(_e) => {} // TODO: Handle errors
            }
        }
    }

    fn write_settings(&self) -> Result<(), TuiError<D::Error>> {
        if let Some(path) = &self.settings_path {
            let state = self.ui_state.deref().borrow();
            // TODO: Have central settings file that lists other databases in order of recent usage.
            // TODO: Write multiple settings files to allow multiple databases.
            let s = Settings {
                interface_style: state.style,
                columns: state
                    .table_view
                    .selected_cols()
                    .iter()
                    .map(|s| s.clone().into_inner())
                    .collect(),
                sort_settings: self.app.sort_settings().clone(),
                navigation_settings: state.nav_settings,
                database_settings: DatabaseSettings {
                    path: self.app.db_path(),
                },
            };
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p)?;
            }
            s.write(path)?;
        }
        Ok(())
    }
}
