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
    text::{Span, Spans},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};

enum Event<I> {
    Input(I),
    Tick,
}

pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    receiver: Receiver<Event<KeyEvent>>,
    table: StatefulTable,
}

impl fmt::Debug for Tui {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<Tui>")
    }
}

#[derive(Debug, Default)]
struct StatefulTable {
    state: TableState,
    row_count: usize,
}

impl StatefulTable {
    fn row_count(&mut self, row_count: usize) {
        self.row_count = row_count;
    }

    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.row_count - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.row_count - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn first(&mut self) {
        self.state.select(Some(0))
    }

    pub fn last(&mut self) {
        self.state.select(Some(self.row_count - 1))
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
            table: StatefulTable::default(),
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
                            if tries.successful > 0 {
                                Cell::from(Spans::from(vec![
                                    Span::styled(
                                        tries.successful.to_string(),
                                        Style::default().fg(Color::Red),
                                    ),
                                    Span::raw(format!("/{}", tries.total)),
                                ]))
                            } else {
                                Cell::from(Span::raw(format!(
                                    "{}/{}",
                                    tries.successful, tries.total
                                )))
                            }
                        }
                        None => Cell::from(Span::raw("0/0")),
                    }
                }
            })
            .collect::<Vec<_>>();
        Row::new(cells)
    }

    fn render_table(
        frame: &mut Frame<CrosstermBackend<Stdout>>,
        rect: Rect,
        stats: &Stats,
        table: &mut StatefulTable,
    ) {
        let table_widget = Table::new(
            stats
                .frequencies
                .iter()
                .map(|(path, method_stats)| Tui::fill_row(path, method_stats))
                .collect::<Vec<Row>>(),
        )
        .header(
            Row::new(vec![
                Cell::from("Path"),
                Cell::from("GET"),
                Cell::from("POST"),
                Cell::from("PUT"),
                Cell::from("PATCH"),
            ])
            .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::White))
                .title("OpenAPI Fuzzer")
                .border_type(BorderType::Plain),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .widths(&[
            Constraint::Percentage(44),
            Constraint::Percentage(14),
            Constraint::Percentage(14),
            Constraint::Percentage(14),
            Constraint::Percentage(14),
        ]);

        frame.render_stateful_widget(table_widget, rect, &mut table.state);
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
        for e in self.receiver.try_iter() {
            match e {
                Event::Input(event) => match event.code {
                    KeyCode::Char('q') => {
                        terminal::disable_raw_mode()?;
                        self.terminal.clear()?;
                        self.terminal.show_cursor()?;
                        return Ok(true);
                    }
                    KeyCode::Down => self.table.next(),
                    KeyCode::Up => self.table.previous(),
                    KeyCode::Home => self.table.first(),
                    KeyCode::End => self.table.last(),
                    _ => {}
                },
                Event::Tick => {}
            };
        }

        let default_message = "Press `q` to quit".to_string();
        self.table.row_count(stats.frequencies.len());
        let table = &mut self.table;
        self.terminal
            .draw(|frame| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(2)
                    .constraints([Constraint::Min(2), Constraint::Length(3)].as_ref())
                    .split(frame.size());

                Tui::render_table(frame, chunks[0], stats, table);
                Tui::render_message_box(
                    frame,
                    chunks[1],
                    message.as_ref().unwrap_or(&default_message),
                );
            })
            .context("unable to draw tui")?;

        Ok(false)
    }
}
