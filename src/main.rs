use core::str;
use std::result::Result::Ok;
use std::{
    io::Write,
    ops::{Neg, Shl, Shr},
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

use column::{combine_number_text, format_automatic, parse_automatic, Column, Cursor};
use format::{char_to_number, hex_to_u8_char, NUMBER_STRING_WIDTH};

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
                        KeyCode::Backspace | KeyCode::Char('x') => self.remove_character()?,
                        KeyCode::Delete => {
                            self.remove_character()?;
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
                            self.tabs[self.tab_index][self.cursor.col as usize].undo();
                            (_, self.cursor) = self.get_current_column();
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
                        KeyCode::Char('<') | KeyCode::Char('l') => {
                            let (num, cur) = self.get_current_column();
                            self.tabs[self.tab_index][self.cursor.col as usize]
                                .set(num.shl(1), cur);
                        }
                        KeyCode::Char('>') | KeyCode::Char('r') => {
                            let (num, cur) = self.get_current_column();
                            self.tabs[self.tab_index][self.cursor.col as usize]
                                .set(num.shr(1), cur);
                        }
                        // rotate left
                        KeyCode::Char('L') => {
                            let (num, cur) = self.get_current_column();
                            self.tabs[self.tab_index][self.cursor.col as usize]
                                .set(num.rotate_left(1), cur);
                        }
                        // rotate right
                        KeyCode::Char('R') => {
                            let (num, cur) = self.get_current_column();
                            self.tabs[self.tab_index][self.cursor.col as usize]
                                .set(num.rotate_right(1), cur);
                        }
                        KeyCode::Char('s') => self.toggle_sign(),
                        KeyCode::Char('-') => self.set_negative(),
                        KeyCode::Char('+') => self.set_positive(),
                        // toggle between insert and replace mode
                        KeyCode::Char('i') => {
                            self.insert = !self.insert;
                        }
                        KeyCode::Char(char) => {
                            if self.insert {
                                self.insert_character(char)?
                            } else {
                                self.replace_character(char)?
                            }
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
                    self.cursor.row = row.clamp(NUMBER_START_Y as u16, NUMBER_START_Y as u16 + 7)
                        as u8
                        - NUMBER_START_Y
                        + 1;
                    let tmp = column.clamp(
                        NUMBER_START_X as u16,
                        NUMBER_START_X as u16 + NUMBER_DIGIT_WIDTH as u16 * 2 + 2,
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
                    // should handle hex or bin prefix (overwrites cursor bosition)
                    let trim = data.as_bytes().trim_ascii();
                    let mut text = [format::CHAR_SPACE; NUMBER_STRING_WIDTH];
                    text[NUMBER_STRING_WIDTH as usize - trim.len()..NUMBER_STRING_WIDTH as usize]
                        .copy_from_slice(trim);
                    eprintln!("{text:?}");
                    if let Ok(number) = parse_automatic(text, self.cursor.row) {
                        self.tabs[self.tab_index][self.cursor.col as usize]
                            .set(number, self.cursor);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn toggle_sign(&mut self) {
        let (num, _) = self.get_current_column();
        let signed = format::handle_negative(num);
        self.tabs[self.tab_index][self.cursor.col as usize]
            .set(signed.neg() as UNumber, self.cursor);
    }

    fn set_positive(&mut self) {
        let (num, _) = self.get_current_column();
        let signed = format::handle_negative(num);
        if signed.is_negative() {
            self.tabs[self.tab_index][self.cursor.col as usize]
                .set(signed.neg() as UNumber, self.cursor);
        }
    }

    fn set_negative(&mut self) {
        let (num, _) = self.get_current_column();
        let signed = format::handle_negative(num);
        if signed.is_positive() {
            self.tabs[self.tab_index][self.cursor.col as usize]
                .set(signed.neg() as UNumber, self.cursor);
        }
    }

    fn replace_character(&mut self, char: char) -> Result<()> {
        let (num, _) = self.get_current_column();

        match self.cursor.row {
            1 | 2 => match char {
                '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                    let mut text = *b",000,000,000,000,000,000,000,000";
                    combine_number_text(&mut text, format_automatic(num, self.cursor.row)?);
                    text[self.cursor.text_pos as usize + 5] = char as u8;
                    let new_number = parse_automatic(text, self.cursor.row)?;
                    self.tabs[self.tab_index][self.cursor.col as usize]
                        .set(new_number, self.cursor);
                }
                _ => {}
            },
            3 => match char {
                '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' | 'A' | 'B' | 'C'
                | 'D' | 'E' | 'F' | 'a' | 'b' | 'c' | 'd' | 'e' | 'f' => {
                    let bit_pos = column::LOOKUP_TABLE[self.cursor.row as usize]
                        [self.cursor.text_pos as usize]
                        * 4;
                    let mut new_number = num & !(0xF << bit_pos);
                    new_number |= char_to_number(char) << bit_pos;
                    self.tabs[self.tab_index][self.cursor.col as usize]
                        .set(new_number, self.cursor);
                }
                _ => {}
            },
            4 | 5 | 6 | 7 => {
                if char == '0' || char == '1' {
                    let bit_pos = column::LOOKUP_TABLE[self.cursor.row as usize]
                        [self.cursor.text_pos as usize];
                    let mask = 1 << bit_pos;
                    let new_number = match char {
                        '1' => num | mask,
                        '0' => num & !mask,
                        _ => num,
                    };
                    self.tabs[self.tab_index][self.cursor.col as usize]
                        .set(new_number, self.cursor);
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn insert_character(&mut self, char: char) -> Result<()> {
        let (num, _) = self.get_current_column();

        match self.cursor.row {
            1 | 2 => match char {
                '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                    let mut text = *b",000,000,000,000,000,000,000,000";
                    combine_number_text(&mut text, format_automatic(num, self.cursor.row)?);
                    let pos = self.cursor.text_pos as usize + 5;
                    // preserve a potential '-' at pos 0
                    text.copy_within(2..pos + 1, 1);
                    text[pos] = char as u8;
                    let new_number = parse_automatic(text, self.cursor.row)?;
                    self.tabs[self.tab_index][self.cursor.col as usize]
                        .set(new_number, self.cursor);
                }
                _ => {}
            },
            3 => match char {
                '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' | 'A' | 'B' | 'C'
                | 'D' | 'E' | 'F' | 'a' | 'b' | 'c' | 'd' | 'e' | 'f' => {
                    if num.leading_zeros() < 4 {
                        self.tabs[self.tab_index][self.cursor.col as usize]
                            .set(UNumber::MAX, self.cursor);
                    } else {
                        let bit_pos = column::LOOKUP_TABLE[self.cursor.row as usize]
                            [self.cursor.text_pos as usize]
                            * 4;
                        let left_mask = UNumber::MAX << bit_pos;
                        let mut new_number = (num & left_mask) << 4;
                        new_number |= num & !left_mask;
                        new_number |= char_to_number(char) << bit_pos;
                        self.tabs[self.tab_index][self.cursor.col as usize]
                            .set(new_number, self.cursor);
                    }
                }
                _ => {}
            },
            4 | 5 | 6 | 7 => {
                if char == '0' || char == '1' {
                    if num.leading_zeros() == 0 {
                        self.tabs[self.tab_index][self.cursor.col as usize]
                            .set(UNumber::MAX, self.cursor);
                    } else {
                        let bit_pos = column::LOOKUP_TABLE[self.cursor.row as usize]
                            [self.cursor.text_pos as usize];
                        let left_mask = UNumber::MAX << bit_pos;
                        let mut new_number = (num & left_mask) << 1;
                        new_number |= num & !left_mask;
                        new_number |= match char {
                            '1' => 1 << bit_pos,
                            _ => 0,
                        };
                        self.tabs[self.tab_index][self.cursor.col as usize]
                            .set(new_number, self.cursor);
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn remove_character(&mut self) -> Result<()> {
        let (num, _) = self.get_current_column();
        match self.cursor.row {
            1 | 2 => {
                let mut text = *b",000,000,000,000,000,000,000,000";
                combine_number_text(&mut text, format_automatic(num, self.cursor.row)?);
                let pos = self.cursor.text_pos as usize + 5;
                text.copy_within(1..pos, 2);
                let new_number = parse_automatic(text, self.cursor.row)?;
                self.tabs[self.tab_index][self.cursor.col as usize].set(new_number, self.cursor);
            }
            3 => {
                let bit_pos = column::LOOKUP_TABLE[self.cursor.row as usize]
                    [self.cursor.text_pos as usize]
                    * 4;
                let left_mask = UNumber::MAX << bit_pos + 4;
                let right_mask = !(UNumber::MAX << bit_pos);
                let mut new_number = (num & left_mask) >> 4;
                new_number |= num & right_mask;
                self.tabs[self.tab_index][self.cursor.col as usize].set(new_number, self.cursor);
            }
            4 | 5 | 6 | 7 => {
                let bit_pos =
                    column::LOOKUP_TABLE[self.cursor.row as usize][self.cursor.text_pos as usize];
                let left_mask = UNumber::MAX << bit_pos + 1;
                let right_mask = !(UNumber::MAX << bit_pos);
                let mut new_number = (num & left_mask) >> 1;
                new_number |= num & right_mask;
                self.tabs[self.tab_index][self.cursor.col as usize].set(new_number, self.cursor);
            }
            _ => {}
        }

        Ok(())
    }

    fn write_trimmed(
        w: &mut Writer,
        text: [u8; NUMBER_STRING_WIDTH],
        col: u8,
        row: u8,
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

        for row in 1..8 {
            Self::write_trimmed(w, format_automatic(number, row)?, col, row + 1)?;
        }
        return Ok(());
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
                text[0] = '/' as u8;
                text[10] = '\\' as u8;
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
