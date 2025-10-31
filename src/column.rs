use anyhow::Result;
use crossterm::{QueueableCommand, cursor};

use crate::{UNumber, Writer};
use std::{ops, time::Instant};

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
// TODO would be fancy to have custom accessor types? (using row enum, and maybe custom type for column????)
pub const LOOKUP_TABLE: [[u8; 28]; 9] = [
    // upper padding
    [
        UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP, UP,
        UP, UP, UP, UP, UP,
    ],
    // decimal
    [
        EL, 19, 18, SC, 17, 16, 15, SC, 14, 13, 12, SC, 11, 10, 9, SC, 8, 7, 6, SC, 5, 4, 3, SC, 2,
        1, 0, ER,
    ],
    // signed
    [
        EL, 19, 18, SC, 17, 16, 15, SC, 14, 13, 12, SC, 11, 10, 9, SC, 8, 7, 6, SC, 5, 4, 3, SC, 2,
        1, 00, ER,
    ],
    // hex
    [
        EL, EL, EL, EL, 15, 14, SC, 13, 12, SC, 11, 10, SC, 9, 8, SC, 7, 6, SC, 5, 4, SC, 3, 2, SC,
        1, 0, ER,
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
        UJ, UJ, UJ, UJ, UJ, UJ, UJ, UJ, 15, 14, 13, 12, SC, 11, 10, 9, 8, SC, 7, 6, 5, 4, SC, 3, 2,
        1, 0, ER,
    ],
    // lower padding
    [
        DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP, DP,
        DP, DP, DP, DP, DP,
    ],
];

// Allow dead code because we construct the enum via ways the static code analysis does not know of
#[allow(dead_code)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Row {
    UpperPadding = 0,
    Decimal,
    Signed,
    Hex,
    Bin0,
    Bin1,
    Bin2,
    Bin3,
    LowerPadding,
}

const LAST_ROW: Row = Row::LowerPadding;

impl TryFrom<u8> for Row {
    type Error = ();

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        if v > LAST_ROW as u8 {
            Err(())
        } else {
            unsafe { std::mem::transmute::<u8, std::result::Result<Self, ()>>(v) }
        }
    }
}

impl TryFrom<usize> for Row {
    type Error = ();

    fn try_from(v: usize) -> Result<Self, Self::Error> {
        if v > LAST_ROW as usize {
            Err(())
        } else {
            unsafe { std::mem::transmute::<u8, std::result::Result<Self, ()>>(v as u8) }
        }
    }
}

impl ops::Add<u8> for Row {
    type Output = Self;

    fn add(self, rhs: u8) -> Self::Output {
        let num = (self as u8).saturating_add(rhs);
        Self::try_from(num).map_or(LAST_ROW, |output| output)
    }
}

impl ops::AddAssign<u8> for Row {
    fn add_assign(&mut self, rhs: u8) {
        *self = *self + rhs;
    }
}

impl ops::SubAssign<u8> for Row {
    fn sub_assign(&mut self, rhs: u8) {
        *self = *self - rhs;
    }
}

impl ops::Sub<u8> for Row {
    type Output = Self;

    fn sub(self, rhs: u8) -> Self::Output {
        let num = (self as u8).saturating_sub(rhs);
        // Row does not have holes, so the unreachable should never happen, assuming the input enum was valid
        Self::try_from(num).unwrap_or_else(|()| unreachable!())
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Cursor {
    pub col: u8,
    pub row: Row,
    // cursor position inside text row
    // This is the visual position, use the above table to derive the rest
    // 1 = leftmost, 26 = rightmost
    pub text_pos: u8,
}

impl Cursor {
    // fix by moving in left direction if possible
    fn fix_left(&mut self) {
        match LOOKUP_TABLE[self.row as usize][self.text_pos as usize] {
            SC | ER => self.text_pos -= 1,
            EL => self.text_pos += 1,
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
            SC | EL => self.text_pos += 1,
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

    pub const fn swap_column(&mut self) {
        self.col += 1;
        self.col %= 2;
    }

    pub fn set_terminal_cursor(self, w: &mut Writer) -> Result<()> {
        let y = self.row as u16 + 1;
        let x = 7 + u16::from(self.col) * 29 + u16::from(self.text_pos);
        w.queue(cursor::MoveTo(x - 1, y))?;
        // Experimenting with changing background color -> changes it for everything in the future
        // w.queue(style::SetBackgroundColor(style::Color::Red))?;
        w.queue(cursor::Show)?;
        w.queue(cursor::MoveTo(x, y))?;
        Ok(())
    }
}

impl Default for Cursor {
    fn default() -> Self {
        // we have 1 layer of padding around the numbers block
        // -> start at 1,1, and end at:26,26
        // the above lookup datle goes to 27,27
        Self {
            col: 0,
            row: Row::Decimal,
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
        let cursor = Cursor {
            col: column_index,
            ..Default::default()
        };
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
        *self
            .history
            .get(self.index)
            .expect("something went wrong with history index math")
    }

    pub const fn undo(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    pub fn redo(&mut self) {
        self.index = self.history.len().min(self.index + 2) - 1;
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
                row: Row::UpperPadding,
                text_pos: 7,
            },
        );

        assert_eq!(
            column.get(),
            (
                42,
                Cursor {
                    col: 3,
                    row: Row::UpperPadding,
                    text_pos: 7,
                }
            )
        );
        assert_eq!(column.history.len(), 2);
        column.set(
            77,
            Cursor {
                col: 9,
                row: Row::UpperPadding,
                text_pos: 5,
            },
        );
        assert_eq!(
            column.get(),
            (
                77,
                Cursor {
                    col: 9,
                    row: Row::UpperPadding,
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
                    row: Row::UpperPadding,
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
                    row: Row::UpperPadding,
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
                    row: Row::UpperPadding,
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
                    row: Row::UpperPadding,
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
                row: Row::UpperPadding,
                text_pos: 1,
            },
        );
        assert_eq!(
            column.get(),
            (
                13,
                Cursor {
                    col: 2,
                    row: Row::UpperPadding,
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
                cursor.row = Row::try_from(r).unwrap();
                cursor.text_pos = c as u8;
                cursor.fix_left();
                cursor.row = Row::try_from(r).unwrap();
                cursor.text_pos = c as u8;
                cursor.fix_right();
            }
        }
        // make sure there are no endless loops
        for r in 1..LOOKUP_TABLE.len() - 1 {
            for c in 1..LOOKUP_TABLE[0].len() - 1 {
                cursor.row = Row::try_from(r).unwrap();
                cursor.text_pos = c as u8;
                cursor.move_left();
                cursor.row = Row::try_from(r).unwrap();
                cursor.text_pos = c as u8;
                cursor.move_right();
                cursor.row = Row::try_from(r).unwrap();
                cursor.text_pos = c as u8;
                cursor.move_up();
                cursor.row = Row::try_from(r).unwrap();
                cursor.text_pos = c as u8;
                cursor.move_down();
            }
        }
    }
    #[test]
    fn test_cursor_movemint() {
        let mut cursor = Cursor::default();
        assert_eq!(cursor.col, 0);
        assert_eq!(cursor.row, Row::Decimal);
        assert_eq!(cursor.text_pos, 26);

        cursor.move_right();
        assert_eq!(cursor.row, Row::Decimal);
        assert_eq!(cursor.text_pos, 26);

        cursor.move_up();
        assert_eq!(cursor.row, Row::Decimal);
        assert_eq!(cursor.text_pos, 26);

        for i in 0..19 {
            cursor.move_left();
            assert_eq!(cursor.row, Row::Decimal);
            assert_eq!(cursor.text_pos, 25 - i - ((i + 1) / 3));
        }

        cursor.move_up();
        assert_eq!(cursor.row, Row::Decimal);
        assert_eq!(cursor.text_pos, 1);
        cursor.move_left();
        assert_eq!(cursor.row, Row::Decimal);
        assert_eq!(cursor.text_pos, 1);

        for i in 0u8..19u8 {
            cursor.move_right();
            assert_eq!(cursor.row, Row::Decimal);
            assert_eq!(cursor.text_pos, 2 + i + i.div_ceil(3));
        }

        assert_eq!(cursor.row, Row::Decimal);
        assert_eq!(cursor.text_pos, 26);
        cursor.text_pos = 1;
        cursor.move_down();
        assert_eq!(cursor.row, Row::Signed);
        assert_eq!(cursor.text_pos, 1);
        cursor.move_down();
        // we are at hex
        assert_eq!(cursor.row, Row::Hex);
        assert_eq!(cursor.text_pos, 4);
        cursor.move_down();
        // we are at bin
        assert_eq!(cursor.row, Row::Bin0);
        assert_eq!(cursor.text_pos, 8);
        cursor.text_pos = 26;
        // test left / right jump down / up
        cursor.move_right();
        assert_eq!(cursor.row, Row::Bin1);
        assert_eq!(cursor.text_pos, 8);
        cursor.move_left();
        assert_eq!(cursor.row, Row::Bin0);
        assert_eq!(cursor.text_pos, 26);

        assert_eq!(cursor.col, 0);
    }
}
