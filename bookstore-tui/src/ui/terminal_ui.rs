use std::marker::PhantomData;
use std::path::PathBuf;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};

use futures::{future::FutureExt, StreamExt};

use tui::backend::Backend;
use tui::layout::Rect;
use tui::Terminal;

use bookstore_app::app::AppChannel;
use bookstore_app::settings::{
    DatabaseSettings, InterfaceSettings, InterfaceStyle, NavigationSettings, Settings, SortSettings,
};
use bookstore_app::table_view::TableView;
use bookstore_app::user_input::{CommandString, EditState};
use bookstore_app::ApplicationError;
use bookstore_database::bookview::BookViewError;
use bookstore_database::{BookView, DatabaseError, IndexableDatabase};

use crate::ui::scrollable_text::ScrollableText;
use crate::ui::views::{
    AppView, ApplicationTask, ColumnWidget, EditWidget, HelpWidget, InputHandler, ResizableWidget,
};
use crate::ui::widgets::{BorderWidget, Widget};

use bookstore_database::paged_cursor::RelativeSelection;

#[derive(Debug)]
pub(crate) enum TuiError<DBError> {
    Application(ApplicationError<DBError>),
    Database(DatabaseError<DBError>),
    Io(std::io::Error),
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

pub(crate) struct UIState<D: IndexableDatabase + Send + Sync> {
    pub(crate) style: InterfaceStyle,
    pub(crate) nav_settings: NavigationSettings,
    pub(crate) curr_command: CommandString,
    pub(crate) selected_column: usize,
    pub(crate) table_view: TableView,
    pub(crate) book_view: BookView<D>,
    pub(crate) sort_settings: SortSettings,
    // pub(crate) command_log: Vec<CommandString>,
}

impl<D: IndexableDatabase + Send + Sync> UIState<D> {
    pub(crate) fn modify_bv(&mut self, f: impl Fn(&mut BookView<D>) -> bool) -> bool {
        f(&mut self.book_view)
    }

    pub(crate) async fn update_column_data(&mut self) -> Result<(), BookViewError<D::Error>> {
        self.table_view.regenerate_columns(&self.book_view).await
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

    pub(crate) async fn make_selection_visible(&mut self) -> Result<(), BookViewError<D::Error>> {
        if self.book_view.make_selection_visible() {
            self.table_view.regenerate_columns(&self.book_view).await?;
        }
        Ok(())
    }
}

trait ViewHandler<D: IndexableDatabase + Send + Sync, B: Backend>:
    ResizableWidget<D, B> + InputHandler<D>
{
}

impl<D: IndexableDatabase + Send + Sync, B: Backend> ViewHandler<D, B> for ColumnWidget<D> {}

impl<D: IndexableDatabase + Send + Sync, B: Backend> ViewHandler<D, B> for EditWidget<D> {}

impl<D: IndexableDatabase + Send + Sync, B: Backend> ViewHandler<D, B> for HelpWidget<D> {}

// TODO: Use channels to allow CTRL+Q when application freezes
//          Also, allow text input / waiting animation
pub(crate) struct AppInterface<'a, D: 'a + IndexableDatabase + Send + Sync, B: Backend> {
    border_widget: BorderWidget,
    active_view: Box<dyn ViewHandler<D, B> + 'a>,
    ui_state: UIState<D>,
    ui_updated: bool,
    settings_path: Option<PathBuf>,
    event_receiver: EventStream,
    app_channel: AppChannel<D>,
}

impl<'a, D: 'a + IndexableDatabase + Send + Sync, B: Backend> AppInterface<'a, D, B> {
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
    pub(crate) async fn new<S: Into<String>>(
        name: S,
        settings: InterfaceSettings,
        settings_path: Option<PathBuf>,
        app_channel: AppChannel<D>,
        event_receiver: EventStream,
        sort_settings: SortSettings,
    ) -> AppInterface<'a, D, B> {
        let book_view = app_channel.new_book_view().await;
        let path = app_channel.db_path().await;
        let ui_state = UIState {
            style: settings.interface_style,
            nav_settings: settings.navigation_settings,
            curr_command: CommandString::new(),
            selected_column: 0,
            table_view: TableView::from(settings.columns),
            book_view,
            sort_settings,
        };
        AppInterface {
            border_widget: BorderWidget::new(name.into(), path),
            active_view: Box::new(ColumnWidget {
                book_widget: None,
                command_widget_selected: false,
                database: Default::default(),
            }),
            ui_updated: false,
            ui_state,
            settings_path,
            app_channel,
            event_receiver,
        }
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
    async fn read_user_input(&mut self) -> Result<bool, TuiError<D::Error>> {
        loop {
            if let Some(Ok(event)) = self.event_receiver.next().fuse().await {
                match event {
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('q'),
                        modifiers: KeyModifiers::CONTROL,
                    }) => return Ok(true),
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('s'),
                        modifiers: KeyModifiers::CONTROL,
                    }) => {
                        self.app_channel.save().await;
                        return Ok(false);
                    }
                    _ => {}
                }
                match self
                    .active_view
                    .handle_input(event, &mut self.ui_state, &mut self.app_channel)
                    .await?
                {
                    ApplicationTask::Quit => return Ok(true),
                    ApplicationTask::SwitchView(view) => {
                        self.ui_updated = true;
                        match view {
                            AppView::Columns => {
                                self.active_view = Box::new(ColumnWidget {
                                    book_widget: None,
                                    command_widget_selected: false,
                                    database: PhantomData,
                                })
                            }
                            AppView::Edit => {
                                let _ = self.ui_state.make_selection_visible().await;
                                if let Some(selected_str) = self.ui_state.selected_table_value() {
                                    self.active_view = Box::new(EditWidget {
                                        edit: EditState::new(selected_str[0]),
                                        focused: true,
                                        database: PhantomData,
                                    });
                                }
                            }
                            AppView::Help(help_string) => {
                                self.active_view = Box::new(HelpWidget {
                                    text: ScrollableText::new(help_string),
                                    database: PhantomData,
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
    pub(crate) async fn run(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<(), TuiError<D::Error>> {
        loop {
            self.ui_state
                .book_view
                .sort_by_columns(&self.ui_state.sort_settings.columns)
                .await?;

            if self.app_channel.take_update().await | self.take_update() {
                self.border_widget.saved = self.app_channel.saved().await;
                let chunk = {
                    let frame = terminal.get_frame();
                    let s = frame.size();
                    Rect::new(
                        s.x + 1,
                        s.y + 1,
                        s.width.saturating_sub(2),
                        s.height.saturating_sub(2),
                    )
                };
                self.active_view
                    .prepare_render(&mut self.ui_state, chunk)
                    .await;
                terminal.draw(|f| {
                    self.border_widget.render_into_frame(f, f.size());
                    self.active_view.render_into_frame(f, &self.ui_state, chunk);
                })?;
            }

            match self.read_user_input().await {
                Ok(true) => {
                    self.write_settings().await?;
                    return Ok(terminal.clear()?);
                }
                Ok(false) => {}
                Err(_e) => {} // TODO: Handle errors
            }
        }
    }

    async fn write_settings(&self) -> Result<(), TuiError<D::Error>> {
        if let Some(path) = &self.settings_path {
            // TODO: Have central settings file that lists other databases in order of recent usage.
            // TODO: Write multiple settings files to allow multiple databases.
            let s = Settings {
                interface_style: self.ui_state.style,
                columns: self
                    .ui_state
                    .table_view
                    .selected_cols()
                    .iter()
                    .map(|s| s.clone().into_inner())
                    .collect(),
                sort_settings: self.ui_state.sort_settings.clone(),
                navigation_settings: self.ui_state.nav_settings,
                database_settings: DatabaseSettings {
                    path: self.app_channel.db_path().await,
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
