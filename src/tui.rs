use std::{
    collections::BTreeMap,
    fmt,
    io::{self, Stdout},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use crate::fuzzer::{Stats, Tries};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEvent},
    terminal,
};
use tui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table},
    Frame, Terminal,
};

enum Event<I> {
    Input(I),
    Tick,
}

pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    receiver: Receiver<Event<KeyEvent>>,
}

impl fmt::Debug for Tui {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<Tui>")
    }
}

impl Tui {
    pub fn new() -> Result<Tui> {
        terminal::enable_raw_mode().context("unable to go to raw mode")?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let (tx, rx) = mpsc::channel();
        let tick_rate = Duration::from_millis(200);
        // Setup event loop for catching key input
        thread::spawn(move || {
            let mut last_tick = Instant::now();
            loop {
                let timeout = tick_rate
                    .checked_sub(last_tick.elapsed())
                    .unwrap_or_else(|| Duration::from_secs(0));

                if event::poll(timeout).expect("poll works") {
                    if let CEvent::Key(key) = event::read().expect("can read events") {
                        tx.send(Event::Input(key)).expect("can send events");
                    }
                }

                if last_tick.elapsed() >= tick_rate {
                    if let Ok(_) = tx.send(Event::Tick) {
                        last_tick = Instant::now();
                    }
                }
            }
        });

        Ok(Tui {
            terminal,
            receiver: rx,
        })
    }

    fn fill_row<'a>(path: &'a str, method_stats: &BTreeMap<String, Tries>) -> Row<'a> {
        let methods = vec!["path", "GET", "POST", "PUT", "PATCH"];

        let cells = methods
            .into_iter()
            .map(|method| {
                if method == "path" {
                    Cell::from(Span::raw(path.clone()))
                } else {
                    match method_stats.get(method) {
                        Some(tries) => {
                            Cell::from(Span::raw(format!("{}/{}", tries.successful, tries.total)))
                        }
                        None => Cell::from(Span::raw("0/0")),
                    }
                }
            })
            .collect::<Vec<_>>();
        Row::new(cells)
    }

    fn render_table(frame: &mut Frame<CrosstermBackend<Stdout>>, rect: Rect, stats: &Stats) {
        let table = Table::new(
            stats
                .frequencies
                .iter()
                .map(|(path, method_stats)| Tui::fill_row(path, method_stats))
                .collect::<Vec<Row>>(),
        )
        .header(Row::new(vec![
            Cell::from(Span::styled(
                "Path",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "GET",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "POST",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "PUT",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "PATCH",
                Style::default().add_modifier(Modifier::BOLD),
            )),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::White))
                .title("OpenAPI Fuzzer")
                .border_type(BorderType::Plain),
        )
        .widths(&[
            Constraint::Percentage(44),
            Constraint::Percentage(14),
            Constraint::Percentage(14),
            Constraint::Percentage(14),
            Constraint::Percentage(14),
        ]);

        frame.render_widget(table, rect)
    }

    fn render_message_box(frame: &mut Frame<CrosstermBackend<Stdout>>, rect: Rect, message: &str) {
        let message = Paragraph::new(message)
            .style(Style::default().fg(Color::LightCyan))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::default().fg(Color::White))
                    .title("Message")
                    .border_type(BorderType::Plain),
            );
        frame.render_widget(message, rect);
    }

    pub fn display(&mut self, stats: &Stats, message: &Option<String>) -> Result<bool> {
        let default_message = "Press `q` to quit".to_string();
        self.terminal
            .draw(|frame| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(2)
                    .constraints([Constraint::Min(2), Constraint::Length(3)].as_ref())
                    .split(frame.size());

                Tui::render_table(frame, chunks[0], stats);
                Tui::render_message_box(
                    frame,
                    chunks[1],
                    message.as_ref().unwrap_or(&default_message),
                );
            })
            .context("unable to draw tui")?;

        match self.receiver.recv()? {
            Event::Input(event) => match event.code {
                KeyCode::Char('q') => {
                    terminal::disable_raw_mode()?;
                    self.terminal.clear()?;
                    self.terminal.show_cursor()?;
                    return Ok(true);
                }
                _ => {}
            },
            Event::Tick => {}
        };
        Ok(false)
    }
}
