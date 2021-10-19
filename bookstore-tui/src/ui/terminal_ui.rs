use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};

use futures::{future::FutureExt, StreamExt};

use tui::backend::Backend;
use tui::layout::Rect;
use tui::Terminal;

use bookstore_app::app::AppChannel;
use bookstore_app::columns::Columns;
use bookstore_app::settings::{
    DatabaseSettings, InterfaceSettings, InterfaceStyle, NavigationSettings, Settings, SortSettings,
};
use bookstore_app::ApplicationError;
use bookstore_database::bookview::BookViewError;
use bookstore_database::{AppDatabase, BookView, DatabaseError};
use bookstore_input::user_input::{CommandString, CommandStringError, InputRecorder};
use bookstore_records::Book;

use crate::ui::scrollable_text::ScrollableText;
use crate::ui::views::{
    AppView, ApplicationTask, ColumnWidget, EditWidget, HelpWidget, InputHandler, ResizableWidget,
};
use crate::ui::widgets::{BorderWidget, Widget};

#[derive(Debug)]
pub(crate) enum TuiError<DBError> {
    Application(ApplicationError<DBError>),
    Database(DatabaseError<DBError>),
    BookView(BookViewError<DBError>),
    Io(std::io::Error),
    CommandString(CommandStringError),
}

impl<DBError> From<ApplicationError<DBError>> for TuiError<DBError> {
    fn from(e: ApplicationError<DBError>) -> Self {
        TuiError::Application(e)
    }
}

impl<DBError> From<BookViewError<DBError>> for TuiError<DBError> {
    fn from(e: BookViewError<DBError>) -> Self {
        TuiError::BookView(e)
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

impl<DBError> From<CommandStringError> for TuiError<DBError> {
    fn from(e: CommandStringError) -> Self {
        TuiError::CommandString(e)
    }
}

pub(crate) struct UIState<D: AppDatabase + Send + Sync + 'static> {
    pub(crate) style: InterfaceStyle,
    pub(crate) nav_settings: NavigationSettings,
    pub(crate) curr_command: CommandString,
    pub(crate) selected_column: usize,
    pub(crate) table_view: Columns,
    pub(crate) book_view: BookView<D>,
    pub(crate) sort_settings: SortSettings,
    // pub(crate) command_log: Vec<CommandString>,
}

impl<D: AppDatabase + Send + Sync> UIState<D> {
    pub(crate) fn selected_column_values(&self) -> Option<Vec<String>> {
        let books: Vec<_> = self
            .book_view
            .relative_selections()
            .into_iter()
            .map(|(_, book)| book)
            .collect();
        let selected_values = self
            .table_view
            .read_columns(&books)
            .nth(self.selected_column)
            .map(|(_, column_values)| column_values.map(String::from).collect());

        selected_values
    }

    pub(crate) fn num_cols(&self) -> usize {
        self.table_view.selected_cols().len()
    }

    pub(crate) fn selected(&self) -> Option<(usize, Vec<(usize, Arc<Book>)>)> {
        Some((self.selected_column, self.book_view.relative_selections()))
    }

    pub(crate) async fn make_selection_visible(&mut self) -> Result<(), BookViewError<D::Error>> {
        self.book_view.refresh().await?;
        Ok(())
    }
}

trait ViewHandler<D: AppDatabase + Send + Sync, B: Backend>:
    ResizableWidget<D, B> + InputHandler<D>
{
}

impl<D: AppDatabase + Send + Sync, B: Backend> ViewHandler<D, B> for ColumnWidget<D> {}

impl<D: AppDatabase + Send + Sync, B: Backend> ViewHandler<D, B> for EditWidget<D> {}

impl<D: AppDatabase + Send + Sync, B: Backend> ViewHandler<D, B> for HelpWidget<D> {}

// TODO: Use channels to allow CTRL+Q when application freezes
//          Also, allow text input / waiting animation
pub(crate) struct AppInterface<'a, D: 'a + AppDatabase + Send + Sync + 'static, B: Backend> {
    border_widget: BorderWidget,
    active_view: Box<dyn ViewHandler<D, B> + 'a>,
    ui_state: UIState<D>,
    ui_updated: bool,
    settings_path: Option<PathBuf>,
    event_receiver: EventStream,
    app_channel: AppChannel<D>,
}

impl<'a, D: 'a + AppDatabase + Send + Sync, B: Backend> AppInterface<'a, D, B> {
    /// Returns a new interface, instantiated with the provided settings and database.
    ///
    /// # Arguments
    ///
    /// * ` name ` - The application instance name. Not to confused with the file name.
    /// * ` settings` - The interface settings.
    /// * ` settings_path ` - The settings path (used to persist settings).
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
            table_view: Columns::from(settings.columns),
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
    /// # Errors
    /// This function may error if executing a particular action fails.
    async fn read_user_input(&mut self) -> Result<bool, TuiError<D::Error>> {
        loop {
            if let Some(Ok(event)) = self.event_receiver.next().fuse().await {
                match event {
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('q'),
                        modifiers: KeyModifiers::CONTROL,
                    }) => return Ok(false),
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('s'),
                        modifiers: KeyModifiers::CONTROL,
                    }) => {
                        self.app_channel.save().await;
                        return Ok(true);
                    }
                    _ => {}
                }
                match self
                    .active_view
                    .handle_input(event, &mut self.ui_state, &mut self.app_channel)
                    .await?
                {
                    ApplicationTask::Quit => return Ok(false),
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
                                let selected_books = self.ui_state.book_view.relative_selections();
                                if let Some(column) = self.ui_state.selected_column_values() {
                                    self.active_view = Box::new(EditWidget {
                                        edit: {
                                            let mut edit = InputRecorder::default();
                                            for ((_, book), col) in
                                                selected_books.into_iter().zip(column.into_iter())
                                            {
                                                edit.add_cursor(book.id(), &col);
                                            }
                                            edit
                                        },
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
        Ok(true)
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
            if self.app_channel.take_update().await | self.take_update() {
                self.border_widget.saved = self.app_channel.saved().await;
                {
                    let frame = terminal.get_frame();
                    let size = frame.size();
                    self.active_view
                        .prepare_render(
                            &mut self.ui_state,
                            Rect::new(
                                size.x + 1,
                                size.y + 1,
                                size.width.saturating_sub(2),
                                size.height.saturating_sub(2),
                            ),
                        )
                        .await;
                };
                terminal.draw(|f| {
                    let size = f.size();
                    if size.height < 2 || size.width < 2 {
                        return;
                    }
                    self.border_widget.render_into_frame(f, size);
                    // TODO: If preparing the render takes too long and the user
                    //  increases the window the window, it is possible for books to not be
                    //  even if they are available, since lazy rendering.
                    tracing::info!("Rendering into terminal with size {:?}", size);
                    self.active_view.render_into_frame(
                        f,
                        &self.ui_state,
                        Rect::new(
                            size.x + 1,
                            size.y + 1,
                            size.width.saturating_sub(2),
                            size.height.saturating_sub(2),
                        ),
                    );
                })?;
            }

            match self.read_user_input().await {
                Ok(false) => {
                    self.write_settings().await?;
                    return Ok(terminal.clear()?);
                }
                Ok(true) => {}
                Err(e) => {
                    tracing::info!("Error occurred during execution: {:?}", e);
                    // TODO: User should be notified when errors occur - but where and how?
                }
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
