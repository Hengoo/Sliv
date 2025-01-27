use core::str;
use std::{io::Write, time::Instant};

use anyhow::{Ok, Result};
use crossterm::{
    cursor,
    event::{
        read, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode,
    },
    execute, style,
    terminal::{self, Clear, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand, SynchronizedUpdate,
};

mod config;
mod format;
mod layout;

// TODO setup clippy

#[derive(Copy, Clone, PartialEq, Debug)]
struct Cursor {
    col: u16,
    // none -> entire row was selected
    row: Option<u16>,
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor { col: 0, row: None }
    }
}

// Numper type used in the hex comparison
// by default we will display u32 but we want to support wider as well.
// everything (also ui) is written to support flexible widths.
type UNumber = u64;
type INumber = i64;

// one value to show and compare
#[derive(Debug)]
struct Column {
    // maybe use circular buffer
    history: Vec<(UNumber, Cursor)>,
    // the index in the history we are currently working with.
    index: usize,
    edit_time: Instant,
}

impl Default for Column {
    fn default() -> Self {
        Self {
            history: vec![(0, Cursor::default())],
            index: 0,
            edit_time: Instant::now(),
        }
    }
}

impl Column {
    fn set(&mut self, number: UNumber, cursor: Cursor) {
        self.index += 1;
        self.history.truncate(self.index);

        self.history.push((number, cursor));
        self.edit_time = Instant::now();
    }

    fn get(&self) -> (UNumber, Cursor) {
        self.history
            .get(self.index)
            .expect("something went wrong with history index math")
            .clone()
    }

    fn undo(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    fn redo(&mut self) {
        self.index = self.history.len().min(self.index + 2) - 1;
    }

    // get type (bin, decimal, hex) depending on config. TODO
    fn get_type(column: Column) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column() {
        let mut column = Column::default();
        assert_eq!(column.get(), (0, Cursor::default()));
        assert_eq!(column.history.len(), 1);
        column.set(
            42,
            Cursor {
                col: 3,
                row: Some(7),
            },
        );

        assert_eq!(
            column.get(),
            (
                42,
                Cursor {
                    col: 3,
                    row: Some(7),
                }
            )
        );
        assert_eq!(column.history.len(), 2);
        column.set(
            77,
            Cursor {
                col: 9,
                row: Some(5),
            },
        );
        assert_eq!(
            column.get(),
            (
                77,
                Cursor {
                    col: 9,
                    row: Some(5),
                }
            )
        );
        assert_eq!(column.history.len(), 3);

        column.undo();
        assert_eq!(
            column.get(),
            (
                42,
                Cursor {
                    col: 3,
                    row: Some(7),
                }
            )
        );
        assert_eq!(column.history.len(), 3);

        column.undo();
        assert_eq!(column.get(), (0, Cursor::default()));
        assert_eq!(column.history.len(), 3);
        column.undo();
        assert_eq!(column.get(), (0, Cursor::default()));
        assert_eq!(column.history.len(), 3);

        column.redo();
        assert_eq!(
            column.get(),
            (
                42,
                Cursor {
                    col: 3,
                    row: Some(7),
                }
            )
        );
        assert_eq!(column.history.len(), 3);

        column.redo();
        assert_eq!(
            column.get(),
            (
                77,
                Cursor {
                    col: 9,
                    row: Some(5),
                }
            )
        );
        assert_eq!(column.history.len(), 3);
        column.redo();
        assert_eq!(
            column.get(),
            (
                77,
                Cursor {
                    col: 9,
                    row: Some(5),
                }
            )
        );
        assert_eq!(column.history.len(), 3);

        column.undo();
        column.undo();
        column.set(
            13,
            Cursor {
                col: 2,
                row: Some(1),
            },
        );
        assert_eq!(
            column.get(),
            (
                13,
                Cursor {
                    col: 2,
                    row: Some(1),
                }
            )
        );
        assert_eq!(column.history.len(), 2);
    }
}

// currently we just have left/right
// I doubt it makes sense to add support for 3 or more due to comparisons
// Maybe I will add taps for that?
const COLUMN_COUNT: usize = 2;

struct State {
    columns: [Column; COLUMN_COUNT],
    // Which column we are currently working on
    column_id: u8,
    // cursor inside the column
    cursor: Cursor,
}

struct App {
    w: std::io::Stdout,
}

impl App {
    fn init() -> Result<App> {
        terminal::enable_raw_mode()?;
        let mut w = std::io::stdout();
        w.execute(EnterAlternateScreen)?
            .execute(EnableBracketedPaste)?
            .execute(EnableMouseCapture)?;
        Ok(App { w })
    }

    fn cleanup(mut self) -> Result<()> {
        self.w
            .execute(LeaveAlternateScreen)?
            .execute(DisableMouseCapture)?
            .execute(DisableBracketedPaste)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }

    // TODO don't forget to queue stuff when it gets more expensive
    fn run(&mut self) -> Result<()> {
        'outer: loop {
            let test: [u8; 2] = ['a' as u8, 'b' as u8];
            let string = str::from_utf8(&test)?;
            self.w
                .execute(terminal::Clear(terminal::ClearType::All))?
                .execute(cursor::MoveTo(5, 5))?
                .execute(crossterm::style::Print("start loop"))?
                .execute(style::Print(string))?
                .execute(cursor::MoveTo(5, 5))?;
            // `read()` blocks until an `Event` is available
            match read()? {
                Event::Key(event) => {
                    write!(self.w, "test {:?}", event)?;
                    write!(self.w, "test {:?}", event.code)?;
                    if event.code == KeyCode::Char('q') {
                        write!(self.w, "what")?;
                        break 'outer;
                    }
                }
                Event::Mouse(event) => write!(self.w, "{:?}", event)?,
                Event::Paste(data) => write!(self.w, "{:?}", data)?,
                Event::Resize(width, height) => write!(self.w, "New size {}x{}", width, height)?,
                _ => {}
            }
        }
        Ok(())
    }
}

/*
// cursed idea: i can have a merged modal editor, meanin supporting hjkl movent?? but clashes with colemark
fn print_events() -> Result<()> {
    execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
    'outer: loop {
        write!(self.w, "start loop");
        // `read()` blocks until an `Event` is available
        match read()? {
            Event::Key(event) => {
                write!(self.w, "test {:?}", event);
                write!(self.w, "test {:?}", event.code);
                if event.code == KeyCode::Char('q') {
                    write!(self.w, "what");
                    break 'outer;
                }
            }
            Event::Mouse(event) => write!(self.w, "{:?}", event),
            Event::Paste(data) => write!(self.w, "{:?}", data),
            Event::Resize(width, height) => write!(self.w, "New size {}x{}", width, height),
            _ => {}
        }
    }
    execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        DisableMouseCapture
    )?;
    Ok(())
}*/

fn main() {
    std::env::set_var("RUST_BACKTRACE", "1");
    // todo have a look at signal handler do correctly handle ctrl-c
    let mut app = App::init().expect("Error during initialization");
    app.run().expect("Error occured");
    app.cleanup().expect("Error during cleanup");
}
