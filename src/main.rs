use core::str;
use std::{
    io::Write,
    ops::{Neg, Shl, Shr},
};

use anyhow::{Ok, Result};
use crossterm::{
    cursor,
    event::{
        read, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, MouseEvent, MouseEventKind,
    },
    style::{self, Stylize},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand, QueueableCommand,
};

use column::{Column, Cursor};
use format::{
    char_to_number, format_binary, format_decimal, format_hexadecimal, format_signed_decimal,
    parse_binary, parse_decimal, parse_hexadecimal, parse_signed_decimal, NUMBER_STRING_WIDTH,
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
    width: u16,
    height: u16,
}

impl App {
    fn init() -> Result<App> {
        terminal::enable_raw_mode()?;
        let mut w = std::io::stdout();
        w.execute(EnterAlternateScreen)?
            .execute(EnableBracketedPaste)?
            .execute(EnableMouseCapture)?;
        let (width, height) = terminal::size()?;

        let left = Column::new(0);
        let right = Column::new(1);

        Ok(App {
            w,
            tabs: vec![[left, right]],
            tab_index: 0,
            cursor: Cursor::default(),
            width,
            height,
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
        Self::redraw_background(&mut self.w)?;
        for c in 0..COLUMN_COUNT {
            let (number, cursor) = self.tabs[self.tab_index][c].get();
            Self::draw_column(&mut self.w, number, cursor.col)?;
        }
        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        self.redraw()?;
        let mut last_tab_index = self.tab_index;
        let mut last_numbers = (
            self.tabs[self.tab_index][0].get().0,
            self.tabs[self.tab_index][1].get().0,
        );
        'outer: loop {
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
                        KeyCode::Backspace => {
                            let mut tmp = self.cursor;
                            tmp.move_left();
                            self.remove_character(tmp)?
                        }
                        // eXecure character (same keybind as in VIM)
                        KeyCode::Char('x') | KeyCode::Delete => {
                            self.remove_character(self.cursor)?
                        }
                        KeyCode::Enter => {}
                        KeyCode::Left => self.cursor.move_left(),
                        KeyCode::Right => self.cursor.move_right(),
                        KeyCode::Up => self.cursor.move_up(),
                        KeyCode::Down => self.cursor.move_down(),
                        KeyCode::Home => self.cursor.swap_column(),
                        KeyCode::End => self.cursor.swap_column(),
                        KeyCode::Tab => {}
                        KeyCode::BackTab => {}
                        KeyCode::Insert => {}
                        // quit
                        KeyCode::Char('q') => break 'outer,
                        // delete
                        KeyCode::Char('d') => {
                            // TODO d is illegal due to hex
                            // self.tabs[self.tab_index][self.cursor.col as usize].set(0, self.cursor)
                        }
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
                        // swap column
                        KeyCode::Char('s') => self.cursor.swap_column(),
                        // yank
                        KeyCode::Char('y') => {
                            // TODO copy to clipboard
                        }
                        // paste TODO (temporary abuse for plus)
                        KeyCode::Char('p') => {
                            let (a, b) = self.get_current_column();
                            self.tabs[self.tab_index][self.cursor.col as usize].set(a + 42, b);
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
                        KeyCode::Char('m') => self.current_toggle_sign(),
                        KeyCode::Char('-') => self.current_toggle_sign(),
                        KeyCode::Char('+') => self.current_set_positive(),
                        // KeyCode::Char(char) => self.handle_replace_character(char)?,
                        KeyCode::Char(char) => self.handle_insert_character(char)?,
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
                    // TODO parsing. Should handle binary / hex prefix
                    write!(self.w, "{:?}", data)?;
                }
                Event::Resize(width, height) => {
                    self.width = width;
                    self.height = height;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn current_toggle_sign(&mut self) {
        let (num, _) = self.get_current_column();
        let signed = format::handle_negative(num);
        self.tabs[self.tab_index][self.cursor.col as usize]
            .set(signed.neg() as UNumber, self.cursor);
    }

    fn current_set_positive(&mut self) {
        let (num, _) = self.get_current_column();
        let signed = format::handle_negative(num);
        if signed.is_negative() {
            self.tabs[self.tab_index][self.cursor.col as usize]
                .set(signed.neg() as UNumber, self.cursor);
        }
    }

    fn format_automatic(number: UNumber, row: u8) -> Result<[u8; NUMBER_STRING_WIDTH]> {
        match row {
            1 => format_decimal(number, true),
            2 => format_signed_decimal(number, true),
            3 => format_hexadecimal(number, true),
            _ => {
                todo!()
            }
        }

        // // bin is split in 4 numbers to fit on screen
        // let mask = u16::MAX as UNumber;
        // let num_partial_row = 4 - (UNumber::BITS - number.leading_zeros()) as u8 / 16;
        // for i in 0..4 {
        //     if i >= num_partial_row {
        //         w.queue(cursor::MoveTo(col as u16, (row + i) as u16))?
        //             .queue(style::Print("       0000 0000 0000 0000"))?;
        //     }
        //     let num = (number >> ((3 - i) * 16)) & mask;
        //     if num != 0 {
        //         Self::write_trimmed(w, format_binary(num, true)?, col, row + i)?;
        //     }
        // }
    }

    fn parse_automatic(text: [u8; NUMBER_STRING_WIDTH], row: u8) -> Result<UNumber> {
        match row {
            1 => parse_decimal(text),
            2 => parse_signed_decimal(text).map(|n| n as UNumber),
            3 => parse_hexadecimal(text),
            _ => {
                todo!()
            }
        }
    }

    fn combine_number_text(left: &mut [u8; NUMBER_STRING_WIDTH], right: [u8; NUMBER_STRING_WIDTH]) {
        let mut is_neg = false;
        for (l, r) in left.iter_mut().zip(right.iter()) {
            if *r == format::CHAR_MINUS {
                is_neg = true;
            } else if !r.is_ascii_whitespace() {
                *l = *r;
            }
        }
        // moves minus to leftmost char to avoid the user writing numbers left of it
        if is_neg {
            left[0] = format::CHAR_MINUS;
        }
    }

    fn handle_replace_character(&mut self, char: char) -> Result<()> {
        let (num, _) = self.get_current_column();

        match self.cursor.row {
            1 | 2 => match char {
                '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                    let mut text = *b",000,000,000,000,000,000,000,000";
                    Self::combine_number_text(
                        &mut text,
                        Self::format_automatic(num, self.cursor.row)?,
                    );
                    text[self.cursor.text_pos as usize + 5] = char as u8;
                    let new_number = Self::parse_automatic(text, self.cursor.row)?;
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

    fn handle_insert_character(&mut self, char: char) -> Result<()> {
        let (num, _) = self.get_current_column();

        match self.cursor.row {
            1 | 2 => match char {
                '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                    let mut text = *b",000,000,000,000,000,000,000,000";
                    Self::combine_number_text(
                        &mut text,
                        Self::format_automatic(num, self.cursor.row)?,
                    );
                    let pos = self.cursor.text_pos as usize + 5;
                    // preserve a potential '-' at pos 0
                    text.copy_within(2..pos + 1, 1);
                    text[pos] = char as u8;
                    let new_number = Self::parse_automatic(text, self.cursor.row)?;
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

    fn remove_character(&mut self, cursor: Cursor) -> Result<()> {
        let (num, _) = self.get_current_column();
        assert_eq!(cursor.col, self.cursor.col);
        match self.cursor.row {
            1 | 2 => {
                let mut text = *b",000,000,000,000,000,000,000,000";
                Self::combine_number_text(&mut text, Self::format_automatic(num, cursor.row)?);
                let pos = cursor.text_pos as usize + 5;
                text.copy_within(1..pos, 2);
                let new_number = Self::parse_automatic(text, self.cursor.row)?;
                self.tabs[self.tab_index][cursor.col as usize].set(new_number, self.cursor);
            }
            3 => {
                let bit_pos =
                    column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize] * 4;
                let left_mask = UNumber::MAX << bit_pos + 4;
                let right_mask = !(UNumber::MAX << bit_pos);
                let mut new_number = (num & left_mask) >> 4;
                new_number |= num & right_mask;
                self.tabs[self.tab_index][cursor.col as usize].set(new_number, self.cursor);
            }
            4 | 5 | 6 | 7 => {
                let bit_pos = column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize];
                let left_mask = UNumber::MAX << bit_pos + 1;
                let right_mask = !(UNumber::MAX << bit_pos);
                let mut new_number = (num & left_mask) >> 1;
                new_number |= num & right_mask;
                self.tabs[self.tab_index][cursor.col as usize].set(new_number, self.cursor);
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
        let mut row = NUMBER_START_Y;
        // decimal
        Self::write_trimmed(w, format_decimal(number, true)?, col, row)?;
        row += 1;
        // signes
        Self::write_trimmed(w, format_signed_decimal(number, true)?, col, row)?;
        row += 1;
        // hex
        Self::write_trimmed(w, format_hexadecimal(number, true)?, col, row)?;
        row += 1;

        // bin is split in 4 numbers to fit on screen
        let mask = u16::MAX as UNumber;
        let num_partial_row = 4 - (UNumber::BITS - number.leading_zeros()) as u8 / 16;
        for i in 0..4 {
            if i >= num_partial_row {
                w.queue(cursor::MoveTo(col as u16, (row + i) as u16))?
                    .queue(style::Print("       0000 0000 0000 0000"))?;
            }
            let num = (number >> ((3 - i) * 16)) & mask;
            if num != 0 {
                Self::write_trimmed(w, format_binary(num, true)?, col, row + i)?;
            }
        }

        Ok(())
    }

    fn redraw_background(w: &mut Writer) -> Result<()> {
        w.queue(terminal::Clear(terminal::ClearType::All))?;
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
