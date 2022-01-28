use std::marker::PhantomData;

use crossterm::event::{Event, KeyCode, KeyModifiers};
use tui::backend::Backend;
use tui::layout::Rect;
use tui::style::{Color, Modifier, Style};
use tui::text::Text;
use tui::widgets::Paragraph;
use tui::Frame;

use bookworm_app::app::AppChannel;
use bookworm_app::parser::Source;
use bookworm_app::{parse_args, Command};
use bookworm_database::{AppDatabase, DatabaseError};

use crate::ui::utils::{
    char_chunks_to_styled_text, copy_from_clipboard, paste_into_clipboard, StyleRules,
};
use crate::ui::widgets::Widget;
use crate::{run_command, ApplicationTask, TuiError, UIState};

use async_trait::async_trait;

pub struct CommandWidget<D> {
    pub(crate) database: PhantomData<fn(D)>,
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> Widget<D, B> for CommandWidget<D> {
    async fn prepare_render(&mut self, _state: &mut UIState<D>, chunk: Rect) {}

    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
        let command_widget = if state.curr_command.is_empty() {
            Paragraph::new(Text::styled(
                "Enter command or search",
                Style::default().add_modifier(Modifier::BOLD),
            ))
        } else {
            let styles = StyleRules::default()
                .add_modifier(Modifier::BOLD)
                .cursor_fg(Color::Black)
                .cursor_bg(Color::White)
                .add_cursor_modifier(Modifier::SLOW_BLINK)
                .selected_fg(Color::White)
                .selected_bg(Color::Blue);

            Paragraph::new(char_chunks_to_styled_text(
                state.curr_command.char_chunks(),
                styles,
            ))
        };
        f.render_widget(command_widget, chunk);
    }

    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        match event {
            Event::Key(event) => {
                // Text input
                match event.code {
                    KeyCode::Backspace => {
                        state.curr_command.backspace();
                    }
                    KeyCode::Char(x) => {
                        if event.modifiers == KeyModifiers::CONTROL {
                            match (x, cfg!(feature = "copypaste")) {
                                ('v', true) => {
                                    if let Some(s) = copy_from_clipboard() {
                                        for c in s.chars() {
                                            state.curr_command.push(c);
                                        }
                                    }
                                }
                                ('c', true) => {
                                    if let Some(text) = state.curr_command.selected() {
                                        paste_into_clipboard(&text.iter().collect::<String>());
                                    } else {
                                        let string = state.curr_command.to_string();
                                        if !string.is_empty() {
                                            paste_into_clipboard(&string);
                                        }
                                    }
                                }
                                ('x', true) => {
                                    if let Some(text) = state.curr_command.selected() {
                                        paste_into_clipboard(&text.iter().collect::<String>());
                                        state.curr_command.del();
                                    } else {
                                        let string = state.curr_command.to_string();
                                        if !string.is_empty() {
                                            paste_into_clipboard(&string);
                                        }
                                        state.curr_command.clear();
                                    }
                                }
                                ('d', _) => {
                                    state.curr_command.deselect();
                                }
                                ('a', _) => state.curr_command.select_all(),
                                _ => state.curr_command.push(x),
                            }
                        } else {
                            state.curr_command.push(x);
                        }
                    }
                    KeyCode::Enter => {
                        // TODO: Below should be part of accept and widgetbox handling
                        // if state.curr_command.is_empty() {
                        //     return if !state.book_view.selected_books().is_empty() {
                        //         Ok(ApplicationTask::SwitchView(AppView::Edit))
                        //     } else {
                        //         self.command_widget_selected = true;
                        //         Ok(ApplicationTask::UpdateUI)
                        //     };
                        // }

                        let args: Vec<_> = state
                            .curr_command
                            .autofilled_values()
                            .into_iter()
                            .map(|(_, a)| a)
                            .collect();

                        state.curr_command.clear();

                        return match parse_args(args) {
                            Ok(command) => match run_command(app, command, state).await? {
                                ApplicationTask::DoNothing => Ok(ApplicationTask::UpdateUI),
                                other => Ok(other),
                            },
                            Err(_) => {
                                // TODO: How should invalid commands be handled?
                                Ok(ApplicationTask::UpdateUI)
                            }
                        };
                    }
                    // if active widget isn't capturing tabs,
                    // capture tab and cycle active widgets
                    KeyCode::Tab | KeyCode::BackTab => {
                        let curr_command = &mut state.curr_command;
                        curr_command.refresh_autofill()?;
                        match parse_args(curr_command.get_values().map(|(_, s)| s).collect()) {
                            Ok(command) => match command {
                                Command::AddBooks(sources) | Command::UpdateBooks(sources) => {
                                    match sources.last() {
                                        Some(Source::File(_)) => {
                                            curr_command.auto_fill(false);
                                        }
                                        Some(Source::Dir(_, _)) => {
                                            curr_command.auto_fill(true);
                                        }
                                        _ => {}
                                    }
                                }
                                _ => {}
                            },
                            Err(_) => {}
                        }
                    }
                    // if active widget, deactivates
                    KeyCode::Esc => state.curr_command.clear(),
                    KeyCode::Delete => state.curr_command.del(),
                    // Scrolling
                    KeyCode::Up => self.select_up(state, event.modifiers).await?,
                    KeyCode::Down => self.select_down(state, event.modifiers).await?,
                    KeyCode::PageDown => self.page_down(state, event.modifiers).await?,
                    KeyCode::PageUp => self.page_up(state, event.modifiers).await?,
                    KeyCode::Home => self.home(state, event.modifiers).await?,
                    KeyCode::End => self.end(state, event.modifiers).await?,
                    KeyCode::Right => {
                        if event.modifiers.intersects(KeyModifiers::SHIFT) {
                            state.curr_command.key_shift_right();
                        } else {
                            state.curr_command.key_right();
                        }
                    }

                    KeyCode::Left => {
                        if event.modifiers.intersects(KeyModifiers::SHIFT) {
                            state.curr_command.key_shift_left();
                        } else {
                            state.curr_command.key_left();
                        }
                    }
                    _ => return Ok(ApplicationTask::DoNothing),
                }
            }
            _ => return Ok(ApplicationTask::UpdateUI),
        }
        Ok(ApplicationTask::UpdateUI)
    }
}

impl<D: AppDatabase + Send + Sync> CommandWidget<D> {
    async fn page_down(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        unimplemented!("Paging down on command widget not supported.");
    }

    async fn page_up(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        unimplemented!("Paging up on command widget not supported.");
    }

    async fn home(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.curr_command.key_shift_up();
        } else {
            state.curr_command.key_up();
        }
        Ok(())
    }

    async fn end(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.curr_command.key_shift_down();
        } else {
            state.curr_command.key_down();
        }
        Ok(())
    }

    async fn select_up(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.curr_command.key_shift_up();
        } else {
            state.curr_command.key_up();
        }
        Ok(())
    }

    async fn select_down(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.curr_command.key_shift_down();
        } else {
            state.curr_command.key_down();
        }
        Ok(())
    }
}
