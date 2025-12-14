#![warn(
    clippy::all,
    // clippy::pedantic,
    clippy::nursery,
    clippy::cargo,
)]

use core::str;
use std::ops::{Shl, Shr};
use std::result::Result::Ok;

use anyhow::{Context, Result};
use arboard::Clipboard;
use crossterm::event::MouseButton;
use crossterm::{
    event::{Event, KeyCode, KeyModifiers, MouseEvent, MouseEventKind, read},
    style::{self},
};

use column::{Column, Cursor, Row};
use format::{
    NUMBER_STRING_WIDTH, format_automatic, hex_to_u8_char, insert_characters_automatic,
    is_valid_character_automatic, parse_user_input, remove_character_automatic,
    replace_characters_automatic,
};
use std::io::Write;

use crate::backend::{Backend, CursorWriteMode};
use crate::column::LAST_ROW;
use crate::format::REAL_NUMBER_STRING_WIDTH;

mod backend;
mod column;
mod format;

// Numper type used in the hex comparison
// UI is designed to handle u64
pub type UNumber = u64;
pub type INumber = i64;

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
    backend: Backend,
    tabs: Vec<[Column; COLUMN_COUNT]>,
    tab_index: usize,
    cursor: Cursor,
    cursor_write_mode: CursorWriteMode,
    write_help: bool,
    force_redraw: bool,
}

impl App {
    fn init() -> Result<Self> {
        let left = Column::new(0);
        let right = Column::new(1);
        let write_mode = CursorWriteMode::Insert;

        let (width, height) = crossterm::terminal::size()?;
        Ok(Self {
            backend: Backend::new(100, 100, width, height, write_mode)?,
            tabs: vec![[left, right]],
            tab_index: 0,
            cursor: Cursor::default(),
            cursor_write_mode: write_mode,
            write_help: false,
            force_redraw: false,
        })
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
        Self::draw_background(&mut self.backend, self.write_help)?;
        self.draw_tabs()?;
        for c in 0..COLUMN_COUNT {
            let (number, cursor) = self.tabs[self.tab_index][c].get();

            if c == self.cursor.col.into() {
                let tmp = if self.cursor.row == Row::Signed {
                    // Avoid stupid edge case with signed numbers
                    // (Otherwise it would always render this as negative number, filling up
                    //  the other rows)
                    let mut copy = self.cursor;
                    copy.row = Row::Decimal;
                    format::replace_characters_automatic(0, copy, b"1")
                } else {
                    format::replace_characters_automatic(0, self.cursor, b"1")
                };
                Self::draw_column(&mut self.backend, tmp, cursor.col, true)?;
            }
            Self::draw_column(&mut self.backend, number, cursor.col, false)?;
        }

        // Float and double is a big edge case -> only here to visualize, not to edit
        //  Not added to row enum to avoid a even bigger mess
        for c in 0..COLUMN_COUNT {
            let col = NUMBER_START_X + c as u8 * (NUMBER_DIGIT_WIDTH + 3);
            let (number, _) = self.tabs[self.tab_index][c].get();

            // Double
            self.write_double(col, NUMBER_START_Y + LAST_ROW as u8 - 1, number)?;
            // Float
            // Second float is the second half of the 32 bits
            let second_half = number.unbounded_shr(32);
            self.write_float(col, NUMBER_START_Y + LAST_ROW as u8, second_half)?;
            // If the float is truncated we highlight the text in yellow
            self.write_float(col, NUMBER_START_Y + LAST_ROW as u8 + 1, number)?;
        }

        // color differences
        let (number_left, _) = self.tabs[self.tab_index][0].get();
        let (number_right, _) = self.tabs[self.tab_index][1].get();
        if number_left == number_right || number_left == 0 || number_right == 0 {
            return Ok(());
        }
        let col_left = NUMBER_START_X;
        let col_right = NUMBER_START_X + NUMBER_DIGIT_WIDTH + 3;
        for row in 0u16..=(Row::LowerPadding as u16) {
            for x in 0u16..u16::from(NUMBER_DIGIT_WIDTH) {
                let y = row + u16::from(NUMBER_START_Y);
                self.backend.set_background_color_if_different(
                    u16::from(col_left) + x,
                    y,
                    u16::from(col_right) + x,
                    y,
                    style::Color::DarkBlue,
                )?;
            }
        }

        self.force_redraw = false;
        Ok(())
    }

    fn write_double(&mut self, col: u8, row: u8, number: u64) -> Result<(), anyhow::Error> {
        let mut text = [b' '; REAL_NUMBER_STRING_WIDTH];
        let double = f64::from_ne_bytes(number.to_ne_bytes());
        if write!(text.as_mut_slice(), "{double:>REAL_NUMBER_STRING_WIDTH$}").is_err() {
            // Fallback to scientific if normal string formating does not fit
            // If https://stackoverflow.com/a/1701085 is correct, then we need at max 24 chars for this
            write!(text.as_mut_slice(), "{double:>REAL_NUMBER_STRING_WIDTH$.e}")
                .context("Stackoverflow was wrong and the scientific representation of a double needs more than 26 chars")?;
        }
        self.backend.cursor_set(u16::from(col), u16::from(row));
        self.backend.print(str::from_utf8(&text)?)?;
        Ok(())
    }

    fn write_float(&mut self, col: u8, row: u8, number: u64) -> Result<(), anyhow::Error> {
        let mut text = [b' '; REAL_NUMBER_STRING_WIDTH];
        let float = f32::from_ne_bytes((number as u32).to_ne_bytes());
        let is_float_truncated = number > u64::from(u32::MAX);
        if write!(text.as_mut_slice(), "{float:>REAL_NUMBER_STRING_WIDTH$}").is_err() {
            // Fallback to scientific if normal string formating does not fit
            write!(text.as_mut_slice(), "{float:>REAL_NUMBER_STRING_WIDTH$.e}")?;
        }
        self.backend.cursor_set(u16::from(col), u16::from(row));
        if is_float_truncated {
            self.backend
                .print_with_color(str::from_utf8(&text)?, style::Color::Yellow)?;
        } else {
            self.backend.print(str::from_utf8(&text)?)?;
        }
        Ok(())
    }

    fn run(&mut self, mut input_numbers: Vec<UNumber>) -> Result<()> {
        if input_numbers.is_empty() {
            // No cmd args provided, lets check if there is a number in the clipboard
            self.paste_from_clipboard(false);
        } else {
            // read all input numbers
            // (Only support max 12 numbers)
            if input_numbers.len() > 12 {
                input_numbers.resize(12, 0);
            }
            self.tabs.resize(
                input_numbers.len().div_ceil(2),
                [Column::new(0), Column::new(1)],
            );
            for (i, num) in input_numbers.into_iter().enumerate() {
                self.tab_index = i / 2;
                self.cursor.col = (i % 2) as u8;
                self.set_number(num);
            }
            self.tab_index = 0;
            self.cursor.col = 0;
        }

        self.redraw()?;
        self.backend.flush(true)?;

        // book keeping of last frames state so we know when to redraw
        let mut last_tab_index = self.tab_index;
        let mut last_numbers = (
            self.tabs[self.tab_index][0].get().0,
            self.tabs[self.tab_index][1].get().0,
        );
        let mut last_cursor = self.cursor;
        'update_loop: loop {
            let current_numbers = (
                self.tabs[self.tab_index][0].get().0,
                self.tabs[self.tab_index][1].get().0,
            );
            // Avoid uneccessary redraws when the screen does not change
            let redraw = self.tab_index != last_tab_index
                || last_numbers != current_numbers
                || last_cursor != self.cursor
                || self.force_redraw;
            if redraw {
                self.redraw()?;
            }

            last_tab_index = self.tab_index;
            last_numbers = current_numbers;
            last_cursor = self.cursor;

            self.cursor.set_terminal_cursor(&mut self.backend);
            self.backend.flush(redraw)?;
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
                            let cursor_before = self.cursor;
                            self.cursor.move_right();
                            // We are already at the right edge -> delete does nothing
                            if cursor_before != self.cursor {
                                let num = remove_character_automatic(num, self.cursor);
                                // Have cursor at the correct position for undo
                                self.cursor = cursor_before;
                                self.set_number(num);
                                self.cursor.move_right();
                            }
                        }
                        KeyCode::Enter => {}
                        KeyCode::Left => {
                            if event.modifiers.contains(KeyModifiers::CONTROL) {
                                self.move_cursor_home()?;
                            } else {
                                self.cursor.move_left();
                            }
                        }
                        KeyCode::Right => {
                            if event.modifiers.contains(KeyModifiers::CONTROL) {
                                self.move_cursor_end();
                            } else {
                                self.cursor.move_right();
                            }
                        }
                        KeyCode::Up => self.cursor.move_up(),
                        KeyCode::Down => self.cursor.move_down(),
                        KeyCode::End => self.move_cursor_end(),
                        KeyCode::Home => self.move_cursor_home()?,
                        KeyCode::Tab | KeyCode::BackTab => {
                            self.cursor.swap_column();
                        }
                        // go tab to right
                        // ctrl -> new tab
                        // alt -> remove tab
                        KeyCode::Char('t') => {
                            if event.modifiers.contains(KeyModifiers::CONTROL) {
                                if self.tabs.len() < 6 {
                                    let left = Column::new(0);
                                    let right = Column::new(1);
                                    self.tabs.push([left, right]);
                                    self.tab_index = self.tabs.len() - 1;
                                }
                            } else if event.modifiers.contains(KeyModifiers::ALT) {
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

                        // quit
                        KeyCode::Char('q' | 'Q') => break 'update_loop,
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
                        KeyCode::Char('y') => self.copy_to_clipboard(false)?,
                        // yank without formatting
                        KeyCode::Char('Y') => self.copy_to_clipboard(true)?,
                        // Ctrl C is copy so it can be done with left hand if needed (exit is q)
                        KeyCode::Char('c') => {
                            if event.modifiers.contains(KeyModifiers::CONTROL) {
                                self.copy_to_clipboard(false)?;
                            } else {
                                self.handle_char_input('c');
                            }
                        }
                        KeyCode::Insert => self.paste_from_clipboard(false),
                        // paste
                        KeyCode::Char('p') => self.paste_from_clipboard(false),
                        // Ctrl V is paste, because why not
                        KeyCode::Char('v') => {
                            if event.modifiers.contains(KeyModifiers::CONTROL) {
                                self.paste_from_clipboard(false);
                            }
                        }
                        // paste at position
                        KeyCode::Char('P') => self.paste_from_clipboard(true),
                        KeyCode::Char('<' | 'l') => {
                            let (num, _) = self.get_current_column();
                            self.set_number(num.shl(1));
                        }
                        KeyCode::Char('>' | 'r') => {
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
                            self.cursor_write_mode = match self.cursor_write_mode {
                                CursorWriteMode::Insert => CursorWriteMode::Replace,
                                CursorWriteMode::Replace => CursorWriteMode::Insert,
                            };
                            self.backend.set_cursor_write_mode(self.cursor_write_mode);
                        }
                        KeyCode::Char('h' | 'H') => {
                            self.write_help = !self.write_help;
                            self.force_redraw = true;
                        }
                        KeyCode::Char(' ') => self.handle_char_input('0'),
                        KeyCode::Char(char) => self.handle_char_input(char),
                        KeyCode::Esc => {}
                        _ => {}
                    }
                }
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    modifiers: _,
                }) => {
                    self.set_cursor_from_mouse(column, row);
                }
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Right),
                    column,
                    row,
                    modifiers: _,
                }) => {
                    self.set_cursor_from_mouse(column, row);
                    self.copy_to_clipboard(false)?;
                }

                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Middle),
                    column,
                    row,
                    modifiers: _,
                }) => {
                    self.set_cursor_from_mouse(column, row);
                    self.paste_from_clipboard(false);
                }
                Event::Paste(data) => {
                    // overwrites current number
                    let (_, cursor) = self.get_current_column();
                    let row = cursor.row;
                    let number = parse_user_input(&data, row);
                    if let Some(number) = number {
                        self.set_number(number);
                    }
                }
                Event::Resize(width, height) => self.backend.update_terminal_size(width, height),
                _ => {}
            }
        }
        Ok(())
    }

    fn set_cursor_from_mouse(&mut self, column: u16, row: u16) {
        self.cursor.row = Row::try_from(
            row.clamp(NUMBER_START_Y.into(), u16::from(NUMBER_START_Y) + 7) as u8 - NUMBER_START_Y
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
        }
        self.cursor.fix_right();
    }

    fn paste_from_clipboard(&mut self, _paste_at_cursor: bool) {
        if let Ok(mut clipboard) = Clipboard::new()
            && let Ok(text) = clipboard.get_text()
            && let Some(number) = parse_user_input(&text, self.cursor.row)
        {
            // TODO This currently always overwrites the entire number, but with paste at cursor
            //  it should use cursor textposition instead and insert / replace accordingly
            self.set_number(number);
        }
    }

    fn copy_to_clipboard(&self, keep_format: bool) -> Result<(), anyhow::Error> {
        // TODO: Optimally I would use the below terminal feature if available, but now idea how to determine that...
        // self.backend.w
        //     .execute(CopyToClipboard::to_primary_from(num.to_string()))?;
        if let Ok(mut clipboard) = Clipboard::new() {
            let (num, _) = self.get_current_column();
            let mut text: Vec<_> = match self.cursor.row {
                Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3 => {
                    let mut res = Vec::with_capacity(usize::from(NUMBER_DIGIT_WIDTH) * 4);
                    res.extend_from_slice(b"0b");
                    res.extend_from_slice(format_automatic(num, Row::Bin0)?.trim_ascii_start());
                    if res.len() != 2 {
                        res.push(b' ');
                    }
                    res.extend_from_slice(format_automatic(num, Row::Bin1)?.trim_ascii_start());
                    if res.len() != 2 {
                        res.push(b' ');
                    }
                    res.extend_from_slice(format_automatic(num, Row::Bin2)?.trim_ascii_start());
                    if res.len() != 2 {
                        res.push(b' ');
                    }
                    res.extend_from_slice(format_automatic(num, Row::Bin3)?.trim_ascii_start());
                    res
                }
                Row::Hex => {
                    let mut res = Vec::with_capacity(usize::from(NUMBER_DIGIT_WIDTH));
                    res.extend_from_slice(b"0x");
                    res.extend_from_slice(format_automatic(num, Row::Hex)?.trim_ascii_start());
                    res
                }
                _ => format_automatic(num, self.cursor.row)?
                    .into_iter()
                    .collect(),
            };
            if !keep_format {
                // remove ',' and ' '
                text.retain(|x| x.is_ascii_alphanumeric() || *x == b'-');
            }
            _ = clipboard.set_text(str::from_utf8(text.as_slice())?);
        }
        Ok(())
    }

    // Move to leftmost position, but not further than the number itself
    fn move_cursor_home(&mut self) -> Result<(), anyhow::Error> {
        let (num, _) = self.get_current_column();
        let text = format_automatic(num, self.cursor.row)?;
        let trimmed = text.trim_ascii_start();
        self.cursor.text_pos = NUMBER_DIGIT_WIDTH - trimmed.len() as u8;
        self.cursor.fix_left();
        Ok(())
    }

    // Move to rightmost position
    fn move_cursor_end(&mut self) {
        self.cursor.move_right();
        self.cursor.text_pos = NUMBER_DIGIT_WIDTH;
        self.cursor.fix_right();
    }

    fn handle_char_input(&mut self, char: char) {
        let (num, _) = self.get_current_column();

        // skip input that is currently not valid
        if !is_valid_character_automatic(char as u8, self.cursor.row) {
            return;
        }

        match self.cursor_write_mode {
            CursorWriteMode::Insert => {
                let num = insert_characters_automatic(num, self.cursor, &[char as u8]);
                self.set_number(num);
            }
            CursorWriteMode::Replace => {
                let num = replace_characters_automatic(num, self.cursor, &[char as u8]);
                self.set_number(num);
            }
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
        b: &mut Backend,
        text: [u8; NUMBER_STRING_WIDTH],
        col: u8,
        row: u8,
        write_background: bool,
    ) -> Result<()> {
        if write_background {
            let mut background = [b' '; NUMBER_STRING_WIDTH];
            for (input, back) in text.iter().zip(background.iter_mut()) {
                if *input != b' ' && *input != b',' {
                    *back = b'0';
                } else {
                    *back = *input;
                }
            }
            // avoid writing leading spaces so we can keep the background
            let trim = background.trim_ascii_start();
            b.cursor_set(
                u16::from(col) + u16::from(NUMBER_DIGIT_WIDTH) - trim.len() as u16,
                u16::from(row),
            );
            b.print_with_color(str::from_utf8(trim)?, COLOR_UNUSED_DIGIT)?;
        } else {
            // avoid writing leading spaces so we can keep the background
            let trim = text.trim_ascii_start();
            b.cursor_set(
                u16::from(col) + u16::from(NUMBER_DIGIT_WIDTH) - trim.len() as u16,
                u16::from(row),
            );
            b.print(str::from_utf8(trim)?)?;
        }

        Ok(())
    }

    fn draw_column(
        b: &mut Backend,
        number: UNumber,
        column_index: u8,
        write_background: bool,
    ) -> Result<()> {
        let col = NUMBER_START_X + column_index * (NUMBER_DIGIT_WIDTH + 3);

        for i in 1u8..8u8 {
            let row = Row::try_from(i).unwrap();
            let text = format_automatic(number, row);
            Self::write_trimmed(b, text?, col, row as u8 + 1, write_background)?;
        }
        Ok(())
    }

    fn draw_tabs(&mut self) -> Result<()> {
        self.backend.cursor_set(0, 0);
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
                self.backend.print(str::from_utf8(&text)?)?;
            } else {
                self.backend
                    .print_with_color(str::from_utf8(&text)?, COLOR_UNUSED_DIGIT)?;
            }
        }

        Ok(())
    }

    fn draw_background(b: &mut Backend, write_help: bool) -> Result<()> {
        // fist row is reserved for tabs
        b.cursor_set(0, u16::from(NUMBER_START_Y) - 1);
        b.println("=================================================================")?;
        b.println("DEC   |                            |                            |")?;
        b.println("SIGNED|                            |                            |")?;
        b.print("HEX   | ")?;
        b.print_with_color("   __ __ __ __ __ __ __ __", COLOR_UNUSED_DIGIT)?;
        b.print(" | ")?;
        b.print_with_color("   __ __ __ __ __ __ __ __", COLOR_UNUSED_DIGIT)?;
        b.print(" |")?;
        b.cursor_move_to_next_line(1);
        b.print("BIN 48| ")?;
        b.print_with_color("       ____ ____ ____ ____", COLOR_UNUSED_DIGIT)?;
        b.print(" | ")?;
        b.print_with_color("       ____ ____ ____ ____", COLOR_UNUSED_DIGIT)?;
        b.print(" |")?;
        b.cursor_move_to_next_line(1);
        b.print("BIN 32| ")?;
        b.print_with_color("       ____ ____ ____ ____", COLOR_UNUSED_DIGIT)?;
        b.print(" | ")?;
        b.print_with_color("       ____ ____ ____ ____", COLOR_UNUSED_DIGIT)?;
        b.print(" |")?;
        b.cursor_move_to_next_line(1);
        b.print("BIN 16| ")?;
        b.print_with_color("       ____ ____ ____ ____", COLOR_UNUSED_DIGIT)?;
        b.print(" | ")?;
        b.print_with_color("       ____ ____ ____ ____", COLOR_UNUSED_DIGIT)?;
        b.print(" |")?;
        b.cursor_move_to_next_line(1);
        b.print("BIN 00| ")?;
        b.print_with_color("       ____ ____ ____ ____", COLOR_UNUSED_DIGIT)?;
        b.print(" | ")?;
        b.print_with_color("       ____ ____ ____ ____", COLOR_UNUSED_DIGIT)?;
        b.print(" |")?;
        b.cursor_move_to_next_line(1);
        b.println("F64   |                            |                            |")?;
        b.println("F32_H |                            |                            |")?;
        b.println("F32_L |                            |                            |")?;
        b.println("=================================================================")?;

        b.print_with_color("Toggle help with 'h'", style::Color::Grey)?;
        b.cursor_move_to_next_line(2);
        if write_help {
            b.println("'q'           quit")?;
            b.println("number (when in hex row also abcdef or ABCDEF) to write number.")?;
            b.println("' '           is treated as '0'")?;
            b.println("'Backspace'   remove character left of cursor")?;
            b.println("'Delete'      remove characters")?;
            b.println("'i'           toggle input mode between insert and replace")?;
            b.println("'u'           undo")?;
            b.println("'U'           redo")?;

            b.cursor_move_to_next_line(1);
            b.println("'>', 'r'      right shift binary")?;
            b.println("'<', 'l'      left  shift binary")?;
            b.println("'R'           right rotate binary")?;
            b.println("'L'           left  rotate binary")?;

            b.cursor_move_to_next_line(1);
            b.println("'s'           toggle sign. Basically number *= -1")?;
            b.println("'-'           set number positive")?;
            b.println("'+'           set number negative")?;

            b.cursor_move_to_next_line(1);
            b.print_with_color(
                "Clipboard (For non obvious format it will prioritize current cursor position):",
                style::Color::Grey,
            )?;
            b.cursor_move_to_next_line(1);
            b.println("'y', 'Ctr_c'  copy number to clipboard")?;
            b.println("'mouse2'      copy number under mouse to clipboard")?;
            b.println("'Y'           copy number with formatting to clipboard")?;
            b.println("'p', 'Ctr_v'  paste number from clipboard")?;
            b.println("'mouse3'      paste number from clipboard to mouse")?;

            b.cursor_move_to_next_line(1);
            b.print_with_color("Cursor movement:", style::Color::Grey)?;
            b.cursor_move_to_next_line(1);
            b.println("'Arrow keys'  cursor movement")?;
            b.println("'mouse1'      cursor movement")?;
            b.println("'Tab'         swap between the two colums")?;
            b.println("'Home', 'Ctr_left'  jump to start of number")?;
            b.println("'End', 'Ctr_right'  jump to end of number")?;

            b.cursor_move_to_next_line(1);
            b.print_with_color(
                "Tabs (See top of the terminal for small preview):",
                style::Color::Grey,
            )?;
            b.cursor_move_to_next_line(1);
            b.println("'Ctr_t'       add new tab")?;
            b.println("'Alt_t'       delete tab")?;
            b.println("'t',          navigate to next tab")?;
            b.println("'T'           navigate to previous tab")?;
        }
        Ok(())
    }
}

fn main() {
    unsafe { std::env::set_var("RUST_BACKTRACE", "1") };

    // Skip the first arg (the executable path)
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(arg) = args.first()
        && (arg == "-h" || arg == "-H" || arg == "-help" || arg == "--help")
    {
        println!("Sliv: Simple Lightweight Integer Visualisation\n");
        println!("Usage: sliv [NUMBER...]");
        println!("       sliv [OPTIONS]\n");
        println!("Number:");
        println!("Any numbers to initialize Sliv with. Can be signed, unsigned, hex, binary.");
        println!("Float not supported");
        println!(
            "When no number is provided Sliv will check if there is a number in the clipboard\n"
        );

        println!("Options:");
        println!("-v, -V, -version, --version         Print version info and exit");
        println!("-h, -H, -help, --help               Print this screen and exit\n");

        println!("While Sliv is running, press 'h' for a keybind overlay\n");
        return;
    }
    if let Some(arg) = args.first()
        && (arg == "-v" || arg == "-V" || arg == "-version" || arg == "--version")
    {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let arg_input: Vec<UNumber> = args
        .into_iter()
        .filter_map(|arg| parse_user_input(&arg, Row::Decimal))
        .collect();

    let mut app = App::init().expect("Error during initialization");
    let res = app.run(arg_input);
    res.expect("App failed somewhere in update loop");
}
