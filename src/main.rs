#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::multiple_crate_versions
)]

use core::str;
use std::ops::{Shl, Shr};
use std::result::Result::Ok;

use anyhow::Result;
use arboard::Clipboard;
use crossterm::event::{KeyEvent, MouseButton};
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

use crate::backend::{Backend, CursorWriteMode};
use crate::column::is_float;
use crate::format::{REAL_NUMBER_STRING_WIDTH, shift_characters_automatic};

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
// Max tabs is an arbitrary limitation to keep UI sane.
const MAX_TABS: usize = 6;

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

    // The buffer we edit floats in before the are "submitted"
    // It is always applied to number, but we need to keep over multiple frames due to float error
    float_buffer: Option<[u8; REAL_NUMBER_STRING_WIDTH]>,
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
            float_buffer: None,
        })
    }

    fn get_current_column(&self) -> (UNumber, Cursor) {
        self.tabs[self.tab_index][self.cursor.col as usize]
            .clone()
            .get()
    }

    fn set_number(&mut self, number: UNumber) {
        let lower_mask = UNumber::from(u32::MAX);
        let upper_mask = !lower_mask;
        match self.cursor.row {
            Row::F32L => {
                let (mut num, _) = self.get_current_column();
                num &= upper_mask;
                num |= number & lower_mask;
                self.tabs[self.tab_index][self.cursor.col as usize].set(num, self.cursor);
            }
            Row::F32H => {
                let (mut num, _) = self.get_current_column();
                num &= lower_mask;
                num |= number & upper_mask;
                self.tabs[self.tab_index][self.cursor.col as usize].set(num, self.cursor);
            }
            _ => self.tabs[self.tab_index][self.cursor.col as usize].set(number, self.cursor),
        }
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

                let overwrite = if self.cursor_is_float() {
                    let mut buffer = [b'1'; REAL_NUMBER_STRING_WIDTH];
                    buffer[..(self.cursor.text_pos - 1) as usize].fill(b' ');
                    Some(buffer)
                } else {
                    None
                };
                Self::draw_column(
                    &mut self.backend,
                    tmp,
                    cursor.col,
                    true,
                    self.cursor,
                    overwrite.as_ref(),
                )?;
            }
            Self::draw_column(
                &mut self.backend,
                number,
                cursor.col,
                false,
                self.cursor,
                self.float_buffer.as_ref(),
            )?;
        }

        // color differences
        let (number_left, _) = self.tabs[self.tab_index][0].get();
        let (number_right, _) = self.tabs[self.tab_index][1].get();
        if number_left == number_right || number_left == 0 || number_right == 0 {
            return Ok(());
        }
        let col_left = NUMBER_START_X;
        let col_right = NUMBER_START_X + NUMBER_DIGIT_WIDTH + 3;
        for row in 0u16..(Row::LowerPadding as u16) {
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

        Ok(())
    }

    fn run(&mut self, mut input_numbers: Vec<UNumber>) -> Result<()> {
        if input_numbers.is_empty() {
            // No cmd args provided, lets check if there is a number in the clipboard
            self.paste_from_clipboard(false);
        } else {
            // read all input numbers
            // Keep tab limit in mind
            if input_numbers.len() > MAX_TABS * 2 {
                input_numbers.resize(MAX_TABS * 2, 0);
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
        let mut last_float_buffer = self.float_buffer;
        'update_loop: loop {
            let current_numbers = (
                self.tabs[self.tab_index][0].get().0,
                self.tabs[self.tab_index][1].get().0,
            );

            // reset float buffer if row, col or tab changed
            if self.tab_index != last_tab_index
                || last_cursor.col != self.cursor.col
                || last_cursor.row != self.cursor.row
            {
                self.float_buffer = None;
            }

            // Avoid uneccessary redraws when the screen does not change
            let redraw = self.tab_index != last_tab_index
                || last_numbers != current_numbers
                || last_cursor != self.cursor
                || last_float_buffer != self.float_buffer
                || self.force_redraw;
            if redraw {
                self.redraw()?;
                self.force_redraw = false;
            }

            last_tab_index = self.tab_index;
            last_numbers = current_numbers;
            last_cursor = self.cursor;
            last_float_buffer = self.float_buffer;
            assert!(!self.force_redraw);

            self.cursor.set_terminal_cursor(&mut self.backend);
            self.backend.flush(redraw)?;
            match read()? {
                Event::Key(event) => {
                    if self.handle_key_event(event)? {
                        break 'update_loop;
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

    // returns true when the app should exit
    fn handle_key_event(&mut self, event: KeyEvent) -> Result<bool> {
        match event.code {
            // eXecure character (same keybind as in VIM)
            KeyCode::Backspace | KeyCode::Char('x') => {
                let ctrl = event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
                self.backspace(ctrl)?;
            }
            KeyCode::Char('X') => {
                self.float_buffer = None;
                self.set_number(0);
                self.cursor.text_pos = NUMBER_DIGIT_WIDTH;
            }
            KeyCode::Delete => {
                let ctrl = event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
                self.delete(ctrl)?;
            }
            KeyCode::Enter => {
                // Used to accept the float buffer
                self.float_buffer = None;
            }
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
                    if self.tabs.len() < MAX_TABS {
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
            KeyCode::Char('q' | 'Q') => {
                return Ok(true);
            }
            // undo
            KeyCode::Char('u') => {
                self.float_buffer = None;
                (_, self.cursor) = self.get_current_column();
                self.tabs[self.tab_index][usize::from(self.cursor.col)].undo();
            }
            // redo
            KeyCode::Char('U') => {
                self.float_buffer = None;
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
                    self.handle_char_input('c')?;
                }
            }
            KeyCode::Insert | KeyCode::Char('p') => self.paste_from_clipboard(false),
            // paste
            // Ctrl V is paste, because why not
            KeyCode::Char('v') => {
                if event.modifiers.contains(KeyModifiers::CONTROL) {
                    self.paste_from_clipboard(false);
                }
            }
            // paste at position
            KeyCode::Char('P') => self.paste_from_clipboard(true),

            KeyCode::Char('<') => {
                let (mut num, _) = self.get_current_column();
                let mut tmp = self.cursor;
                tmp.row = match tmp.row {
                    Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3 => Row::Bin3,
                    _ => tmp.row,
                };
                tmp.text_pos = Cursor::default().text_pos;
                num = shift_characters_automatic(num, tmp, 1);
                self.set_number(num);
            }
            KeyCode::Char('>') => {
                let (mut num, _) = self.get_current_column();
                let mut tmp = self.cursor;
                tmp.row = match tmp.row {
                    Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3 => Row::Bin3,
                    _ => tmp.row,
                };
                tmp.text_pos = Cursor::default().text_pos;
                num = shift_characters_automatic(num, tmp, -1);
                self.set_number(num);
            }
            KeyCode::Char('l') => {
                let (num, _) = self.get_current_column();
                self.set_number(num.shl(1));
            }
            KeyCode::Char('r') => {
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
                if event.modifiers.contains(KeyModifiers::CONTROL) {
                    // Terminals are cooked, Ctrl+Backspace reaches us as Ctrl+h
                    self.backspace(true)?;
                } else {
                    self.write_help = !self.write_help;
                    self.force_redraw = true;
                }
            }
            KeyCode::Char(' ') => {
                let _ = self.handle_char_input('0')?;
            }
            KeyCode::Char('f' | 'F') => {
                if self.cursor_is_float() {
                    self.set_number(parse_user_input("inf", self.cursor.row).unwrap());
                    self.float_buffer = None;
                } else {
                    let _ = self.handle_char_input('f')?;
                }
            }
            KeyCode::Char('n' | 'N') => {
                if self.cursor_is_float() {
                    self.set_number(parse_user_input("nan", self.cursor.row).unwrap());
                    self.float_buffer = None;
                } else {
                    let _ = self.handle_char_input('n')?;
                }
            }
            KeyCode::Char(char) => {
                let _ = self.handle_char_input(char)?;
            }
            _ => {}
        }
        Ok(false)
    }

    fn delete(&mut self, to_word_end: bool) -> Result<(), anyhow::Error> {
        if self.cursor_is_float() {
            let i = (self.cursor.text_pos - 1) as usize;
            if i < REAL_NUMBER_STRING_WIDTH - 1 {
                if self.float_buffer.is_none() {
                    self.init_float_buffer()?;
                }
                let buffer = self.float_buffer.as_mut().unwrap();
                let shift_distance = if to_word_end { buffer.len() - i - 1 } else { 1 };
                buffer.copy_within(0..=i, shift_distance);
                buffer[0..shift_distance].fill(b' ');
                self.apply_float_buffer()?;
                if to_word_end {
                    self.move_cursor_end();
                } else {
                    self.cursor.move_right();
                }
            }
        } else {
            let (num, _) = self.get_current_column();
            let mut del_cursor = self.cursor;
            del_cursor.move_right();
            // We are already at the right edge -> delete does nothing
            if del_cursor == self.cursor {
                return Ok(());
            }
            if to_word_end {
                let mut num = num;
                let mut last_num = num;
                let mut last_cursor = del_cursor;
                loop {
                    num = remove_character_automatic(num, del_cursor);
                    del_cursor.move_right();
                    if num == last_num || last_cursor == del_cursor {
                        break;
                    }
                    last_num = num;
                    last_cursor = del_cursor;
                }
                self.set_number(num);
                loop {
                    self.move_cursor_end();
                    if !matches!(self.cursor.row, Row::Bin0 | Row::Bin1 | Row::Bin2) {
                        break;
                    }
                }
            } else {
                let num = remove_character_automatic(num, del_cursor);
                self.set_number(num);
                self.cursor.move_right();
            }
        }
        Ok(())
    }

    fn backspace(&mut self, to_word_end: bool) -> Result<(), anyhow::Error> {
        if self.cursor_is_float() {
            if self.float_buffer.is_none() {
                self.init_float_buffer()?;
            }
            let buffer = self.float_buffer.as_mut().unwrap();
            let i = (self.cursor.text_pos - 1) as usize;
            buffer.copy_within(0..i, 1);
            let clear_id = if to_word_end { i + 1 } else { 1 };
            buffer[0..clear_id].fill(b' ');
            self.apply_float_buffer()?;
        } else {
            let (mut num, _) = self.get_current_column();
            let mut before = num;
            loop {
                num = remove_character_automatic(num, self.cursor);
                if num == before || !to_word_end {
                    break;
                }
                before = num;
            }
            self.set_number(num);
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
        let text = self.float_buffer.map_or_else(
            || format_automatic(num, self.cursor.row),
            |t| {
                let mut res = [b' '; NUMBER_STRING_WIDTH];
                res[NUMBER_STRING_WIDTH - REAL_NUMBER_STRING_WIDTH..].copy_from_slice(&t);
                Ok(res)
            },
        )?;

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

    fn apply_float_buffer(&mut self) -> Result<()> {
        if let Some(num) = parse_user_input(
            str::from_utf8(self.float_buffer.as_ref().unwrap())?,
            self.cursor.row,
        ) {
            self.set_number(num);
        }
        Ok(())
    }

    // Returns true when the character was a valid key
    fn handle_char_input(&mut self, char: char) -> Result<bool> {
        let (num, _) = self.get_current_column();

        // skip input that is currently not valid
        if !is_valid_character_automatic(char as u8, self.cursor.row) {
            return Ok(false);
        }

        if self.cursor_is_float() {
            if self.float_buffer.is_none() {
                self.init_float_buffer()?;
            }
            let mut buffer = [b'0'; REAL_NUMBER_STRING_WIDTH];
            buffer[..(self.cursor.text_pos) as usize].fill(b' ');
            let mut trimmed = self.float_buffer.as_ref().unwrap().trim_ascii_start();
            let negative = trimmed.starts_with(b"-");
            if negative {
                trimmed = &trimmed[1..];
            }
            buffer[REAL_NUMBER_STRING_WIDTH - trimmed.len()..].copy_from_slice(trimmed);

            let i = (self.cursor.text_pos - 1) as usize;
            if matches!(self.cursor_write_mode, CursorWriteMode::Insert) {
                buffer.copy_within(1..=i, 0);
            }
            buffer[i] = char as u8;
            if i != 0 && negative {
                buffer[i - 1] = b'-';
            }
            if matches!(self.cursor_write_mode, CursorWriteMode::Replace) {
                self.cursor.move_right();
            }

            self.float_buffer = Some(buffer);
            self.apply_float_buffer()?;
            if i == 0 && negative {
                // fallback when user is writing leftmost char on negative float
                self.set_negative();
            }
            return Ok(true);
        }

        match self.cursor_write_mode {
            CursorWriteMode::Insert => {
                let num = insert_characters_automatic(num, self.cursor, &[char as u8]);
                self.set_number(num);
            }
            CursorWriteMode::Replace => {
                let num = replace_characters_automatic(num, self.cursor, &[char as u8]);
                self.set_number(num);
                self.cursor.move_right();
            }
        }
        Ok(true)
    }

    const fn cursor_is_float(&self) -> bool {
        is_float(self.cursor.row)
    }

    fn init_float_buffer(&mut self) -> Result<()> {
        let (num, _) = self.get_current_column();
        let tmp = format_automatic(num, self.cursor.row)?;
        let mut buffer = [b' '; REAL_NUMBER_STRING_WIDTH];
        buffer.copy_from_slice(&tmp[NUMBER_STRING_WIDTH - REAL_NUMBER_STRING_WIDTH..]);
        self.float_buffer = Some(buffer);
        Ok(())
    }

    fn toggle_sign(&mut self) {
        let (num, _) = self.get_current_column();

        match self.cursor.row {
            // for the floats we just flip the signed bit
            Row::F64 | Row::F32H => self.set_number(num ^ 0x8000_0000_0000_0000),
            Row::F32L => self.set_number(num ^ 0x8000_0000),
            _ => {
                let signed = format::handle_negative(num);
                if let Some(neg) = signed.checked_neg() {
                    self.set_number(neg.cast_unsigned());
                } else {
                    // this can only happen if singed num was MIN
                    self.set_number((INumber::MAX) as UNumber + 1);
                }
            }
        }
        self.float_buffer = None;
    }

    fn set_positive(&mut self) {
        let (num, _) = self.get_current_column();
        match self.cursor.row {
            Row::F64 | Row::F32H => self.set_number(num & !0x8000_0000_0000_0000),
            Row::F32L => self.set_number(num & !0x8000_0000),
            _ => {
                let signed = format::handle_negative(num);
                if signed.is_negative() {
                    self.toggle_sign();
                }
            }
        }
        self.float_buffer = None;
    }

    fn set_negative(&mut self) {
        let (num, _) = self.get_current_column();
        match self.cursor.row {
            Row::F64 | Row::F32H => self.set_number(num | 0x8000_0000_0000_0000),
            Row::F32L => self.set_number(num | 0x8000_0000),
            _ => {
                let signed = format::handle_negative(num);
                if signed.is_positive() {
                    self.toggle_sign();
                }
            }
        }
        self.float_buffer = None;
    }

    fn write_trimmed(
        b: &mut Backend,
        text: [u8; NUMBER_STRING_WIDTH],
        col: u8,
        row: u8, // this is NOT the ROW, but the row in the terminal
        write_background: bool,
        color: Option<style::Color>,
    ) -> Result<()> {
        // Background is refering to the gray zeroes indicating what would change when you write a number
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

            if let Some(color) = color {
                b.print_with_color(str::from_utf8(trim)?, color)?;
            } else {
                b.print(str::from_utf8(trim)?)?;
            }
        }

        Ok(())
    }

    fn draw_column(
        b: &mut Backend,
        number: UNumber,
        col: u8,
        write_background: bool,
        current_cursor: Cursor,
        overwrite: Option<&[u8; REAL_NUMBER_STRING_WIDTH]>,
    ) -> Result<()> {
        let x_pos = NUMBER_START_X + col * (NUMBER_DIGIT_WIDTH + 3);

        for i in 1u8..11u8 {
            let row = Row::try_from(i).unwrap();

            let text = if row == current_cursor.row
                && col == current_cursor.col
                && let Some(overwrite) = overwrite
            {
                let mut text = [b' '; NUMBER_STRING_WIDTH];
                text[NUMBER_STRING_WIDTH - REAL_NUMBER_STRING_WIDTH..].copy_from_slice(overwrite);
                text
            } else {
                format_automatic(number, row)?
            };

            // grey zero backround cursor hint
            if write_background {
                let mut skip = false;
                // disable hint for rows the cursor is not on
                if matches!(row, Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3) {
                    skip |= !matches!(
                        current_cursor.row,
                        Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3
                    );
                } else {
                    skip |= row != current_cursor.row;
                }
                if skip {
                    continue;
                }
                Self::write_trimmed(
                    b,
                    text,
                    x_pos,
                    row as u8 + 1,
                    write_background,
                    Some(COLOR_UNUSED_DIGIT),
                )?;
                continue;
            }
            Self::write_trimmed(b, text, x_pos, row as u8 + 1, write_background, None)?;
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
            b.println("'Backspace', 'x'   remove character left of cursor")?;
            b.println("'X'           set number to zero")?;
            b.println("'Delete'      remove characters")?;
            b.println("'i'           toggle input mode between insert and replace")?;
            b.println("'u'           undo")?;
            b.println("'U'           redo")?;

            b.cursor_move_to_next_line(1);
            b.println("'>'           right shift")?;
            b.println("'<'           left  shift")?;
            b.println("'r'           right shift binary")?;
            b.println("'l'           left  shift binary")?;
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
            b.println("'y', 'Ctr+c'  copy number to clipboard")?;
            b.println("'mouse2'      copy number under mouse to clipboard")?;
            b.println("'Y'           copy number with formatting to clipboard")?;
            b.println("'p', 'Ctr+v'  paste number from clipboard")?;
            b.println("'mouse3'      paste number from clipboard to mouse")?;

            b.cursor_move_to_next_line(1);
            b.print_with_color("Cursor movement:", style::Color::Grey)?;
            b.cursor_move_to_next_line(1);
            b.println("'Arrow keys'  cursor movement")?;
            b.println("'mouse1'      cursor movement")?;
            b.println("'Tab'         swap between the two colums")?;
            b.println("'Home', 'Ctr+left'  jump to start of number")?;
            b.println("'End', 'Ctr+right'  jump to end of number")?;

            b.cursor_move_to_next_line(1);
            b.print_with_color(
                "Tabs (See top of the terminal for small preview):",
                style::Color::Grey,
            )?;
            b.cursor_move_to_next_line(1);
            b.println("'Ctr+t'       add new tab")?;
            b.println("'Alt+t'       delete tab")?;
            b.println("'t',          navigate to next tab")?;
            b.println("'T'           navigate to previous tab")?;

            b.cursor_move_to_next_line(1);
            b.print_with_color("Float specifics:", style::Color::Grey)?;
            b.cursor_move_to_next_line(1);
            b.println("'f'           Set to infinity")?;
            b.println("'n'           Set to NaN")?;
            b.println("'Enter'       Accept the temporary float number")?;
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
