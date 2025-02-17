use anyhow::{anyhow, Result};
use crossterm::{cursor, QueueableCommand};

use crate::{UNumber, Writer};
use std::time::Instant;

use crate::format::{
    format_binary, format_decimal, format_hexadecimal, format_signed_decimal, parse_binary,
    parse_decimal, parse_hexadecimal, parse_signed_decimal, NUMBER_STRING_WIDTH,
};

// We are not using enums to keep the table somewhat readable

// SeparatorCell in lookup table
const SC: u8 = u8::MAX;
// EndLeft
const EL: u8 = u8::MAX - 1;
// EndRight
const ER: u8 = u8::MAX - 2;
// UpJump: binary number continues in the row above
const UJ: u8 = u8::MAX - 3;
// DownJump: binary number continues in the row below
const DJ: u8 = u8::MAX - 4;
// UpperPadding
const UP: u8 = u8::MAX - 5;
// DownPadding
const DP: u8 = u8::MAX - 6;
// table has padding in all directions to simplify edge cases
pub const LOOKUP_TABLE: [[u8; 28]; 9] = [
    // upper padding
    [
        UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP,
        UP, UP, UP, UP, UP,
    ],
    // decimal
    [
        EL, 19, 18, SC, 17, 16, 15, SC, 14, 13, 12, SC, 11, 10, 09, SC, 08, 07, 06, SC, 05, 04, 03,
        SC, 02, 01, 00, ER,
    ],
    // signed
    [
        EL, 19, 18, SC, 17, 16, 15, SC, 14, 13, 12, SC, 11, 10, 09, SC, 08, 07, 06, SC, 05, 04, 03,
        SC, 02, 01, 00, ER,
    ],
    // hex
    [
        EL, EL, EL, EL, 15, 14, SC, 13, 12, SC, 11, 10, SC, 09, 08, SC, 07, 06, SC, 05, 04, SC, 03,
        02, SC, 01, 00, ER,
    ],
    // bin
    [
        EL, EL, EL, EL, EL, EL, EL, EL, 63, 62, 61, 60, SC, 59, 58, 57, 56, SC, 55, 54, 53, 52, SC,
        51, 50, 49, 48, DJ,
    ],
    // bin
    [
        UJ, UJ, UJ, UJ, UJ, UJ, UJ, UJ, 47, 46, 45, 44, SC, 43, 42, 41, 40, SC, 39, 38, 37, 36, SC,
        35, 34, 33, 32, DJ,
    ],
    // bin
    [
        UJ, UJ, UJ, UJ, UJ, UJ, UJ, UJ, 31, 30, 29, 28, SC, 27, 26, 25, 24, SC, 23, 22, 21, 20, SC,
        19, 18, 17, 16, DJ,
    ],
    // bin
    [
        UJ, UJ, UJ, UJ, UJ, UJ, UJ, UJ, 15, 14, 13, 12, SC, 11, 10, 09, 08, SC, 07, 06, 05, 04, SC,
        03, 02, 01, 00, ER,
    ],
    // lower padding
    [
        DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP,
        DP, DP, DP, DP, DP,
    ],
];

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Cursor {
    pub col: u8,
    pub row: u8,
    // cursor position inside text row
    // This is the visual position, use the above table to derive the rest
    // 1 = leftmost, 26 = rightmost
    pub text_pos: u8,
}

impl Cursor {
    // fix by moving in left direction if possible
    fn fix_left(&mut self) {
        match LOOKUP_TABLE[self.row as usize][self.text_pos as usize] {
            SC => self.text_pos -= 1,
            EL => self.text_pos += 1,
            ER => self.text_pos -= 1,
            UJ => {
                self.text_pos = 26;
                self.row -= 1;
            }
            DJ => {
                self.text_pos -= 1;
            }
            UP => self.row += 1,
            DP => self.row -= 1,
            _ => {
                return;
            }
        }
        self.fix_left();
    }

    // fix by moving in right direction if possible
    pub fn fix_right(&mut self) {
        match LOOKUP_TABLE[self.row as usize][self.text_pos as usize] {
            SC => self.text_pos += 1,
            EL => self.text_pos += 1,
            ER => self.text_pos -= 1,
            UJ => {
                self.text_pos += 1;
            }
            DJ => {
                self.text_pos = 1;
                self.row += 1;
            }
            UP => self.row += 1,
            DP => self.row -= 1,
            _ => {
                return;
            }
        }
        self.fix_right();
    }

    pub fn move_left(&mut self) {
        self.text_pos -= 1;
        self.fix_left();
    }

    pub fn move_right(&mut self) {
        self.text_pos += 1;
        self.fix_right();
    }

    pub fn move_up(&mut self) {
        self.row -= 1;
        self.fix_right();
    }

    pub fn move_down(&mut self) {
        self.row += 1;
        self.fix_right();
    }

    pub fn swap_column(&mut self) {
        self.col += 1;
        self.col %= 2;
    }

    pub fn set_terminal_cursor(&self, w: &mut Writer) -> Result<()> {
        let y = self.row as u16 + 1;
        let x = 7 + self.col as u16 * 29 + self.text_pos as u16;
        w.queue(cursor::MoveTo(x, y))?;
        Ok(())
    }
}

impl Default for Cursor {
    fn default() -> Self {
        // we have 1 layer of padding around the numbers block
        // -> start at 1,1, and end at:26,26
        // the above lookup datle goes to 27,27
        Cursor {
            col: 0,
            row: 1,
            text_pos: 26,
        }
    }
}

// one value to show and compare
#[derive(Debug, Clone)]
pub struct Column {
    // each column has its own history
    // maybe use circular buffer
    history: Vec<(UNumber, Cursor)>,
    // the index in the history we are currently working with.
    index: usize,
    edit_time: Instant,
}

impl Column {
    pub fn new(column_index: u8) -> Self {
        let mut cursor = Cursor::default();
        cursor.col = column_index;
        Self {
            history: vec![(0, cursor)],
            index: 0,
            edit_time: Instant::now(),
        }
    }
    pub fn set(&mut self, number: UNumber, cursor: Cursor) {
        let (prev_number, _) = self.get();
        if prev_number == number {
            return;
        }
        self.index += 1;
        self.history.truncate(self.index);

        // TODO some logic to merge history entries
        //  recent edit, cursor not jumpet
        self.history.push((number, cursor));
        self.edit_time = Instant::now();
    }

    pub fn get(&self) -> (UNumber, Cursor) {
        self.history
            .get(self.index)
            .expect("something went wrong with history index math")
            .clone()
    }

    pub fn undo(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    pub fn redo(&mut self) {
        self.index = self.history.len().min(self.index + 2) - 1;
    }
}

pub fn combine_number_text(left: &mut [u8; NUMBER_STRING_WIDTH], right: [u8; NUMBER_STRING_WIDTH]) {
    let mut is_neg = false;
    for (l, r) in left.iter_mut().zip(right.iter()) {
        if *r == crate::format::CHAR_MINUS {
            is_neg = true;
        } else if !r.is_ascii_whitespace() {
            *l = *r;
        }
    }
    // moves minus to leftmost char to avoid the user writing numbers left of it
    if is_neg {
        left[0] = crate::format::CHAR_MINUS;
    }
}

// Format depending on the row
// 1 = decimal
// 2 = signed
// 3 = hex
// 4, 5, 6, 7 = combined binary
pub fn format_automatic(number: UNumber, row: u8) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    match row {
        1 => format_decimal(number),
        2 => format_signed_decimal(number),
        3 => format_hexadecimal(number),
        4 | 5 | 6 | 7 => {
            // bin is split in 4 numbers to fit on screen
            let mask = u16::MAX as UNumber;
            let num_partial_row = 4 - (UNumber::BITS - number.leading_zeros()) as u8 / 16;
            let i = row - 4;
            let mut text = if i >= num_partial_row {
                *b"             0000 0000 0000 0000"
            } else {
                *b"                                "
            };
            let num = (number >> ((3 - i) * 16)) & mask;
            if num != 0 || row == 7 {
                combine_number_text(&mut text, format_binary(num)?);
            }
            Ok(text)
        }
        _ => Err(anyhow!("Wrong row?")),
    }
}

// Parse depending on the row
// 1 = decimal
// 2 = signed
// 3 = hex
// 4, 5, 6, 7 = combined binary
pub fn parse_automatic(text: [u8; NUMBER_STRING_WIDTH], row: u8) -> Result<UNumber> {
    match row {
        1 => parse_decimal(text),
        2 => parse_signed_decimal(text).map(|n| n as UNumber),
        3 => parse_hexadecimal(text),
        4 | 5 | 6 | 7 => parse_binary(text),
        _ => Err(anyhow!("Wrong row?")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column() {
        let mut column = Column::new(0);
        assert_eq!(column.get(), (0, Cursor::default()));
        assert_eq!(column.history.len(), 1);
        column.set(
            42,
            Cursor {
                col: 3,
                row: 0,
                text_pos: 7,
            },
        );

        assert_eq!(
            column.get(),
            (
                42,
                Cursor {
                    col: 3,
                    row: 0,
                    text_pos: 7,
                }
            )
        );
        assert_eq!(column.history.len(), 2);
        column.set(
            77,
            Cursor {
                col: 9,
                row: 0,
                text_pos: 5,
            },
        );
        assert_eq!(
            column.get(),
            (
                77,
                Cursor {
                    col: 9,
                    row: 0,
                    text_pos: 5,
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
                    row: 0,
                    text_pos: 7,
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
                    row: 0,
                    text_pos: 7,
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
                    row: 0,
                    text_pos: 5,
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
                    row: 0,
                    text_pos: 5,
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
                row: 0,
                text_pos: 1,
            },
        );
        assert_eq!(
            column.get(),
            (
                13,
                Cursor {
                    col: 2,
                    row: 0,
                    text_pos: 1,
                }
            )
        );
        assert_eq!(column.history.len(), 2);
    }

    #[test]
    fn test_cursor_fix() {
        let mut cursor = Cursor::default();
        // make sure it can recover from every field
        for r in 0..LOOKUP_TABLE.len() {
            for c in 0..LOOKUP_TABLE[0].len() {
                cursor.row = r as u8;
                cursor.text_pos = c as u8;
                cursor.fix_left();
                cursor.row = r as u8;
                cursor.text_pos = c as u8;
                cursor.fix_right();
            }
        }
        // make sure there are no endless loops
        for r in 1..LOOKUP_TABLE.len() - 1 {
            for c in 1..LOOKUP_TABLE[0].len() - 1 {
                cursor.row = r as u8;
                cursor.text_pos = c as u8;
                cursor.move_left();
                cursor.row = r as u8;
                cursor.text_pos = c as u8;
                cursor.move_right();
                cursor.row = r as u8;
                cursor.text_pos = c as u8;
                cursor.move_up();
                cursor.row = r as u8;
                cursor.text_pos = c as u8;
                cursor.move_down();
            }
        }
    }
    #[test]
    fn test_cursor_movemint() {
        let mut cursor = Cursor::default();
        assert_eq!(cursor.col, 0);
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.text_pos, 26);

        cursor.move_right();
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.text_pos, 26);

        cursor.move_up();
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.text_pos, 26);

        for i in 0..19 {
            cursor.move_left();
            assert_eq!(cursor.row, 1);
            assert_eq!(cursor.text_pos, 25 - i - ((i + 1) / 3));
        }

        cursor.move_up();
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.text_pos, 1);
        cursor.move_left();
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.text_pos, 1);

        for i in 0..19 {
            cursor.move_right();
            assert_eq!(cursor.row, 1);
            assert_eq!(cursor.text_pos, 2 + i + ((i + 2) / 3));
        }

        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.text_pos, 26);
        cursor.text_pos = 1;
        cursor.move_down();
        assert_eq!(cursor.row, 2);
        assert_eq!(cursor.text_pos, 1);
        cursor.move_down();
        // we are at hex
        assert_eq!(cursor.row, 3);
        assert_eq!(cursor.text_pos, 4);
        cursor.move_down();
        // we are at bin
        assert_eq!(cursor.row, 4);
        assert_eq!(cursor.text_pos, 8);
        cursor.text_pos = 26;
        // test left / right jump down / up
        cursor.move_right();
        assert_eq!(cursor.row, 5);
        assert_eq!(cursor.text_pos, 8);
        cursor.move_left();
        assert_eq!(cursor.row, 4);
        assert_eq!(cursor.text_pos, 26);

        assert_eq!(cursor.col, 0);
    }
}
