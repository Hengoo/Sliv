use core::str;
use std::result::Result::Ok;
use std::{
    io::Write,
    ops::{Shl, Shr},
};

use anyhow::Result;
use crossterm::{
    cursor,
    event::{
        read, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyModifiers, MouseEvent, MouseEventKind,
    },
    style::{self, Stylize},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand, QueueableCommand,
};

use column::{Column, Cursor, Row};
use format::{
    format_automatic, hex_to_u8_char, insert_characters_automatic, parse_automatic,
    remove_character_automatic, replace_characters_automatic, NUMBER_STRING_WIDTH,
};

mod column;
mod format;

// TODO setup clippy

// Numper type used in the hex comparison
// UI is designed to handle u64
pub type UNumber = u64;
pub type INumber = i64;

// not sure if i need to change this at some point.
type Writer = std::io::Stdout;

// currently we just have left/right
// I doubt it makes sense to add support for 3 or more due to comparisons
// Maybe I will add taps for that?
const COLUMN_COUNT: usize = 2;

const NUMBER_START_X: u8 = 8;
// one row for tabs, one for horizontal line
const NUMBER_START_Y: u8 = 2;
const NUMBER_DIGIT_WIDTH: u8 = 26;

const COLOR_UNUSED_DIGIT: style::Color = style::Color::DarkGrey;

#[derive(Debug)]
struct App {
    w: Writer,
    tabs: Vec<[Column; COLUMN_COUNT]>,
    tab_index: usize,
    cursor: Cursor,
    insert: bool,
}

impl App {
    fn init() -> Result<App> {
        terminal::enable_raw_mode()?;
        let mut w = std::io::stdout();
        w.execute(EnterAlternateScreen)?
            .execute(EnableBracketedPaste)?
            .execute(EnableMouseCapture)?;

        let left = Column::new(0);
        let right = Column::new(1);

        Ok(App {
            w,
            tabs: vec![[left, right]],
            tab_index: 0,
            cursor: Cursor::default(),
            insert: true,
        })
    }

    fn cleanup(mut self) -> Result<()> {
        self.w.flush()?;
        self.w
            .execute(LeaveAlternateScreen)?
            .execute(DisableMouseCapture)?
            .execute(DisableBracketedPaste)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }

    fn get_current_column(&self) -> (UNumber, Cursor) {
        self.tabs[self.tab_index][self.cursor.col as usize]
            .clone()
            .get()
    }

    fn set_number(&mut self, number: UNumber) {
        self.tabs[self.tab_index][self.cursor.col as usize].set(number, self.cursor);
    }

    fn redraw(&mut self) -> Result<()> {
        self.w.queue(terminal::Clear(terminal::ClearType::All))?;
        Self::draw_background(&mut self.w)?;
        self.draw_tabs()?;
        for c in 0..COLUMN_COUNT {
            let (number, cursor) = self.tabs[self.tab_index][c].get();
            Self::draw_column(&mut self.w, number, cursor.col)?;
        }
        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        self.redraw()?;

        // book keebing of last frames state so we know when to redraw
        let mut last_tab_index = self.tab_index;
        let mut last_numbers = (
            self.tabs[self.tab_index][0].get().0,
            self.tabs[self.tab_index][1].get().0,
        );
        'update_loop: loop {
            let current_numbers = (
                self.tabs[self.tab_index][0].get().0,
                self.tabs[self.tab_index][1].get().0,
            );
            // everything must be in one big queue, otherwise it seems we get flickering
            if self.tab_index != last_tab_index || last_numbers != current_numbers {
                self.redraw()?;
            }

            last_tab_index = self.tab_index;
            last_numbers = current_numbers;

            self.cursor.set_terminal_cursor(&mut self.w)?;

            self.w.flush()?;
            match read()? {
                Event::Key(event) => {
                    match event.code {
                        // eXecure character (same keybind as in VIM)
                        KeyCode::Backspace | KeyCode::Char('x') => {
                            let (num, _) = self.get_current_column();
                            let num = remove_character_automatic(num, self.cursor);
                            self.set_number(num);
                        }
                        KeyCode::Delete => {
                            let (num, _) = self.get_current_column();
                            let num = remove_character_automatic(num, self.cursor);
                            self.set_number(num);
                            self.cursor.move_right();
                        }
                        KeyCode::Enter => {}
                        KeyCode::Left => self.cursor.move_left(),
                        KeyCode::Right => self.cursor.move_right(),
                        KeyCode::Up => self.cursor.move_up(),
                        KeyCode::Down => self.cursor.move_down(),
                        KeyCode::Home => self.cursor.swap_column(),
                        KeyCode::End => self.cursor.swap_column(),
                        KeyCode::Tab => {
                            self.cursor.swap_column();
                        }
                        KeyCode::BackTab => {
                            self.cursor.swap_column();
                        }
                        // go tab to right
                        // ctrl -> new tab
                        // alt -> remove tab
                        KeyCode::Char('t') => {
                            if event.modifiers.intersects(KeyModifiers::CONTROL) {
                                let left = Column::new(0);
                                let right = Column::new(1);
                                self.tabs.push([left, right]);
                                self.tab_index = self.tabs.len() - 1;
                            } else if event.modifiers.intersects(KeyModifiers::ALT) {
                                self.tabs.remove(self.tab_index);
                                if self.tabs.is_empty() {
                                    let left = Column::new(0);
                                    let right = Column::new(1);
                                    self.tabs.push([left, right]);
                                }
                                self.tab_index = self.tab_index.min(self.tabs.len() - 1);
                            } else {
                                self.tab_index += 1;
                                self.tab_index %= self.tabs.len();
                            }
                        }
                        // go tab to left
                        KeyCode::Char('T') => {
                            self.tab_index = self.tab_index.wrapping_sub(1);
                            self.tab_index = self.tab_index.min(self.tabs.len() - 1);
                        }

                        KeyCode::Insert => {}
                        // quit
                        KeyCode::Char('q') => break 'update_loop,
                        // undo
                        KeyCode::Char('u') => {
                            (_, self.cursor) = self.get_current_column();
                            self.tabs[self.tab_index][usize::from(self.cursor.col)].undo();
                        }
                        // redo
                        KeyCode::Char('U') => {
                            self.tabs[self.tab_index][self.cursor.col as usize].redo();
                            (_, self.cursor) = self.get_current_column();
                        }
                        // yank
                        KeyCode::Char('y') => {
                            // TODO copy to clipboard
                        }
                        // paste
                        KeyCode::Char('p') => {
                            // TODO paste from clipboard
                        }
                        // paste at position
                        KeyCode::Char('P') => {
                            // TODO paste from clipboard
                            // paste the numbers at position (guess char by char. Make sure history is not polluted)
                        }
                        KeyCode::Char('<') | KeyCode::Char('l') => {
                            let (num, _) = self.get_current_column();
                            self.set_number(num.shl(1));
                        }
                        KeyCode::Char('>') | KeyCode::Char('r') => {
                            let (num, _) = self.get_current_column();
                            self.set_number(num.shr(1));
                        }
                        // rotate left
                        KeyCode::Char('L') => {
                            let (num, _) = self.get_current_column();
                            self.set_number(num.rotate_left(1));
                        }
                        // rotate right
                        KeyCode::Char('R') => {
                            let (num, _) = self.get_current_column();
                            self.set_number(num.rotate_right(1));
                        }
                        KeyCode::Char('s') => self.toggle_sign(),
                        KeyCode::Char('-') => self.set_negative(),
                        KeyCode::Char('+') => self.set_positive(),
                        // toggle between insert and replace mode
                        KeyCode::Char('i') => {
                            self.insert = !self.insert;
                        }
                        KeyCode::Char(' ') => {
                            self.handle_char_input('0');
                        }
                        KeyCode::Char(char) => {
                            self.handle_char_input(char);
                        }
                        KeyCode::Esc => {}
                        _ => {}
                    }
                }
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(_),
                    column,
                    row,
                    modifiers: _,
                }) => {
                    self.cursor.row = Row::try_from(
                        row.clamp(NUMBER_START_Y.into(), u16::from(NUMBER_START_Y) + 7) as u8
                            - NUMBER_START_Y
                            + 1,
                    )
                    .unwrap();
                    let tmp = column.clamp(
                        NUMBER_START_X.into(),
                        u16::from(NUMBER_START_X) + u16::from(NUMBER_DIGIT_WIDTH) * 2 + 2,
                    ) as u8
                        - NUMBER_START_X
                        + 1;
                    if tmp <= NUMBER_DIGIT_WIDTH + 2 {
                        self.cursor.text_pos = tmp.clamp(1, 26);
                        self.cursor.col = 0;
                    } else {
                        self.cursor.text_pos = (tmp - NUMBER_DIGIT_WIDTH - 3).clamp(1, 26);
                        self.cursor.col = 1;
                    };
                    self.cursor.fix_right();
                }

                Event::Paste(data) => {
                    // TODO should be helper funcition
                    // should handle hex or bin prefix (overwrites cursor position)
                    let trim = data.as_bytes().trim_ascii();
                    let (num, _) = self.get_current_column();
                    let num = replace_characters_automatic(num, self.cursor, trim);
                    self.set_number(num);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_char_input(&mut self, char: char) {
        if self.insert {
            let (num, _) = self.get_current_column();
            let num = insert_characters_automatic(num, self.cursor, &[char as u8]);
            self.set_number(num);
        } else {
            let (num, _) = self.get_current_column();
            let num = replace_characters_automatic(num, self.cursor, &[char as u8]);
            self.set_number(num);
        }
    }

    fn toggle_sign(&mut self) {
        let (num, _) = self.get_current_column();
        let signed = format::handle_negative(num);
        if let Some(neg) = signed.checked_neg() {
            self.set_number(neg as UNumber);
        } else {
            // this can only happen if singed num was MIN
            self.set_number((INumber::MAX) as UNumber + 1);
        }
    }

    fn set_positive(&mut self) {
        let (num, _) = self.get_current_column();
        let signed = format::handle_negative(num);
        if signed.is_negative() {
            self.toggle_sign();
        }
    }

    fn set_negative(&mut self) {
        let (num, _) = self.get_current_column();
        let signed = format::handle_negative(num);
        if signed.is_positive() {
            self.toggle_sign();
        }
    }

    fn write_trimmed(
        w: &mut Writer,
        text: [u8; NUMBER_STRING_WIDTH],
        col: u8,
        row: Row,
    ) -> Result<()> {
        // avoid writing leading spaces so we can keep the background
        let trim = text.trim_ascii_start();
        w.queue(cursor::MoveTo(
            col as u16 + NUMBER_DIGIT_WIDTH as u16 - trim.len() as u16,
            row as u16,
        ))?
        .queue(style::Print(str::from_utf8(trim)?))?;

        Ok(())
    }

    fn draw_column(w: &mut Writer, number: UNumber, column_index: u8) -> Result<()> {
        let col = NUMBER_START_X + column_index * { NUMBER_DIGIT_WIDTH + 3 };

        for i in 1u8..8u8 {
            let row = Row::try_from(i).unwrap();
            let text = format_automatic(number, row);
            // Signed is allowed to fail
            if row == Row::Signed {
                if let Ok(text) = text {
                    Self::write_trimmed(w, text, col, row + 1)?;
                }
            }
            Self::write_trimmed(w, text?, col, row + 1)?;
        }
        Ok(())
    }

    fn draw_tabs(&mut self) -> Result<()> {
        let w = &mut self.w;
        w.queue(cursor::MoveTo(0, 0))?;
        for t in 0..self.tabs.len() {
            let mut text = *b"     |     ";
            let [left, right] = &self.tabs[t];
            let (n, _) = left.get();
            text[1] = hex_to_u8_char(n, 12);
            text[2] = hex_to_u8_char(n, 8);
            text[3] = hex_to_u8_char(n, 4);
            text[4] = hex_to_u8_char(n, 0);

            let (n, _) = right.get();
            text[6] = hex_to_u8_char(n, 12);
            text[7] = hex_to_u8_char(n, 8);
            text[8] = hex_to_u8_char(n, 4);
            text[9] = hex_to_u8_char(n, 0);
            if t == self.tab_index {
                text[0] = b'/';
                text[10] = b'\\';
                w.queue(style::Print(str::from_utf8(&text)?))?;
            } else {
                w.queue(style::PrintStyledContent(
                    str::from_utf8(&text)?.with(COLOR_UNUSED_DIGIT),
                ))?;
            }
        }

        Ok(())
    }

    fn draw_background(w: &mut Writer) -> Result<()> {
        // fist row is reserved for tabs
        w.queue(cursor::MoveTo(0, NUMBER_START_Y as u16 - 1))?
            .queue(style::Print(
                "=================================================================",
            ))?
            .queue(cursor::MoveToNextLine(1))?
            .queue(style::Print(
                "DEC   |                            |                            |",
            ))?
            .queue(cursor::MoveToNextLine(1))?
            .queue(style::Print(
                "SIGNED|                            |                            |",
            ))?
            .queue(cursor::MoveToNextLine(1))?
            .queue(style::Print("HEX   | "))?
            .queue(style::PrintStyledContent(
                "   xx xx xx xx xx xx xx xx".with(COLOR_UNUSED_DIGIT),
            ))?
            .queue(style::Print(" | "))?
            .queue(style::PrintStyledContent(
                "   xx xx xx xx xx xx xx xx".with(COLOR_UNUSED_DIGIT),
            ))?
            .queue(style::Print(" |"))?
            .queue(cursor::MoveToNextLine(1))?
            .queue(style::Print("BIN 48| "))?
            .queue(style::PrintStyledContent(
                "       xxxx xxxx xxxx xxxx".with(COLOR_UNUSED_DIGIT),
            ))?
            .queue(style::Print(" | "))?
            .queue(style::PrintStyledContent(
                "       xxxx xxxx xxxx xxxx".with(COLOR_UNUSED_DIGIT),
            ))?
            .queue(style::Print(" |"))?
            .queue(cursor::MoveToNextLine(1))?
            .queue(style::Print("BIN 32| "))?
            .queue(style::PrintStyledContent(
                "       xxxx xxxx xxxx xxxx".with(COLOR_UNUSED_DIGIT),
            ))?
            .queue(style::Print(" | "))?
            .queue(style::PrintStyledContent(
                "       xxxx xxxx xxxx xxxx".with(COLOR_UNUSED_DIGIT),
            ))?
            .queue(style::Print(" |"))?
            .queue(cursor::MoveToNextLine(1))?
            .queue(style::Print("BIN 16| "))?
            .queue(style::PrintStyledContent(
                "       xxxx xxxx xxxx xxxx".with(COLOR_UNUSED_DIGIT),
            ))?
            .queue(style::Print(" | "))?
            .queue(style::PrintStyledContent(
                "       xxxx xxxx xxxx xxxx".with(COLOR_UNUSED_DIGIT),
            ))?
            .queue(style::Print(" |"))?
            .queue(cursor::MoveToNextLine(1))?
            .queue(style::Print("BIN 00| "))?
            .queue(style::PrintStyledContent(
                "       xxxx xxxx xxxx xxxx".with(COLOR_UNUSED_DIGIT),
            ))?
            .queue(style::Print(" | "))?
            .queue(style::PrintStyledContent(
                "       xxxx xxxx xxxx xxxx".with(COLOR_UNUSED_DIGIT),
            ))?
            .queue(style::Print(" |"))?;
        Ok(())
    }
}

fn main() {
    std::env::set_var("RUST_BACKTRACE", "1");
    // TODO have a look at signal handler do correctly handle ctrl-c

    let mut app = App::init().expect("Error during initialization");
    let res = app.run();
    app.cleanup().expect("Error during cleanup");
    res.expect("App failed somewhere in update loop");
}
