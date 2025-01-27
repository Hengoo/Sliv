use crossterm::{execute, queue, style, ExecutableCommand, QueueableCommand};

use crate::{INumber, UNumber};
use core::str;
use std::{
    fmt::{self, format},
    io::Write,
    ops::BitAnd,
};

use anyhow::{Ok, Result};

// How many digits the longest number needs to be displayed, including padding inside of the number.
// This is NOT including padding to left / right sides
const MAX_NUMBER_WIDTH: usize = 32;

const CHAR_0: u8 = '0' as u8;
const CHAR_1: u8 = '1' as u8;
const CHAR_2: u8 = '2' as u8;
const CHAR_3: u8 = '3' as u8;
const CHAR_4: u8 = '4' as u8;
const CHAR_5: u8 = '5' as u8;
const CHAR_6: u8 = '6' as u8;
const CHAR_7: u8 = '7' as u8;
const CHAR_8: u8 = '8' as u8;
const CHAR_9: u8 = '9' as u8;
const CHAR_A: u8 = 'A' as u8;
const CHAR_B: u8 = 'B' as u8;
const CHAR_C: u8 = 'C' as u8;
const CHAR_D: u8 = 'D' as u8;
const CHAR_E: u8 = 'D' as u8;
const CHAR_F: u8 = 'F' as u8;
const CHAR_NUMBER: [u8; 16] = [
    CHAR_0, CHAR_1, CHAR_2, CHAR_3, CHAR_4, CHAR_5, CHAR_6, CHAR_7, CHAR_8, CHAR_9, CHAR_A, CHAR_B,
    CHAR_C, CHAR_D, CHAR_E, CHAR_F,
];
const CHAR_SPACE: u8 = ' ' as u8;
const CHAR_COMMA: u8 = ',' as u8;

// decimals have a ',' every 3 digits
// returns (how many characters were written, the string)
pub fn format_decimal_old(number: UNumber) -> (u32, [u8; MAX_NUMBER_WIDTH]) {
    let mut tmp = number;
    let mut index = 0;
    let mut separator_index = 0;
    let mut result = [0; MAX_NUMBER_WIDTH];
    while tmp >= 10 {
        result[index] = CHAR_NUMBER[(tmp % 10) as usize];
        tmp /= 10;
        index += 1;
        separator_index += 1;
        if separator_index == 3 {
            result[index] = CHAR_COMMA;
            index += 1;
            separator_index = 0;
        }
    }
    result[index] = CHAR_NUMBER[tmp as usize];
    index += 1;
    (index as u32, result)
}

fn handle_negative(number: UNumber) -> (bool, INumber) {
    if number == 0 {
        return (false, number as INumber);
    }

    let base = number.ilog2();
    if let 7 | 15 | 31 | 63 | 127 = base {
        (true, (number & !(2 as UNumber).pow(base)) as INumber * -1)
    } else {
        (false, number as INumber)
    }
}

// Writes from left to right, returning how many bytes were written
// requires that str only contains 8 bit characters. This is not validated.
// everything is queued and not flushed
pub fn write_with_separator<W>(w: &mut W, text: &str, separator: char, digits: u32) -> Result<u32>
where
    W: Write + ?Sized,
{
    let mut written_count = 0;
    let mut rest;
    // handle negative numbers, minus should not be included in any padding calculation
    if text.starts_with('-') {
        w.queue(style::Print('-'))?;
        rest = text.split_at(1).1;
        written_count += 1;
    } else {
        rest = text;
    }

    // handle the first section that might not have full number of characters
    let count = rest.len();
    let mut first_section = count % digits as usize;
    if first_section == 0 {
        first_section = count.min(digits as usize);
    }
    let left;
    (left, rest) = rest.split_at(first_section);
    w.queue(style::Print(left))?;
    written_count += first_section as u32;

    let iterations = rest.len() / digits as usize;
    written_count += iterations as u32 * (digits + 1);

    // go over the rest
    for _ in 0..iterations {
        w.queue(style::Print(separator))?;
        let left;
        (left, rest) = rest.split_at(digits as usize);
        w.queue(style::Print(left))?;
    }
    return Ok(written_count);
}

// returns the width needed to represent a number of a certai bit width in our formating
// this function might be massive overkill and could be a constant instead?
pub fn compute_width(bit_width: u32) -> Result<u32> {
    if bit_width == 0 {
        return Ok(0);
    }
    assert!(bit_width <= UNumber::BITS);
    // not 100% accurate, but does not change result
    let positive = (2 as UNumber).saturating_pow(bit_width) - 1;
    let (is_negative, negative) = handle_negative(positive);

    println!("{}, {},", positive, negative);

    let mut text = [CHAR_SPACE; MAX_NUMBER_WIDTH];
    let mut text_sep = [CHAR_SPACE; MAX_NUMBER_WIDTH];

    write!(text.as_mut_slice(), "{}", positive)?;
    println!("{:?}", text);
    let pos_width = write_with_separator(
        &mut text_sep.as_mut_slice(),
        str::from_utf8(&text)?.trim(),
        ',',
        3,
    )?;

    let mut neg_with = 0;
    if is_negative {
        let mut text = [CHAR_SPACE; MAX_NUMBER_WIDTH];
        write!(text.as_mut_slice(), "{}", negative)?;
        neg_with = write_with_separator(
            &mut text_sep.as_mut_slice(),
            str::from_utf8(&text)?.trim(),
            ',',
            3,
        )?;
    }
    Ok(pos_width.max(neg_with))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_witdh() {
        std::env::set_var("RUST_BACKTRACE", "1");
        assert_eq!(compute_width(0).unwrap(), 0);
        assert_eq!(compute_width(8).unwrap(), 4);
        assert_eq!(compute_width(12).unwrap(), 5);
        assert_eq!(compute_width(16).unwrap(), 7);
        assert_eq!(compute_width(32).unwrap(), 14);
    }

    #[test]
    fn test_decimal_format() {
        let mut text = [CHAR_SPACE; MAX_NUMBER_WIDTH];
        let mut count = write_with_separator(&mut text.as_mut_slice(), "123456", '_', 2).unwrap();
        assert_eq!(
            String::from_utf8_lossy(text.split_at(count as usize).0),
            "12_34_56"
        );
        count = write_with_separator(&mut text.as_mut_slice(), "123456", '_', 2).unwrap();
        assert_eq!(
            String::from_utf8_lossy(text.split_at(count as usize).0),
            "12_34_56"
        );

        count = write_with_separator(&mut text.as_mut_slice(), "-123456", '_', 2).unwrap();
        assert_eq!(
            String::from_utf8_lossy(text.split_at(count as usize).0),
            "-12_34_56"
        );

        count = write_with_separator(&mut text.as_mut_slice(), "123456", '_', 1).unwrap();
        assert_eq!(
            String::from_utf8_lossy(text.split_at(count as usize).0),
            "1_2_3_4_5_6"
        );

        count = write_with_separator(&mut text.as_mut_slice(), "13", '_', 3).unwrap();
        assert_eq!(
            String::from_utf8_lossy(text.split_at(count as usize).0),
            "13"
        );

        count = write_with_separator(&mut text.as_mut_slice(), "12", '_', 2).unwrap();
        assert_eq!(
            String::from_utf8_lossy(text.split_at(count as usize).0),
            "12"
        );

        count = write_with_separator(&mut text.as_mut_slice(), "-12", '_', 2).unwrap();
        assert_eq!(
            String::from_utf8_lossy(text.split_at(count as usize).0),
            "-12"
        );

        count = write_with_separator(&mut text.as_mut_slice(), "-12", '_', 5).unwrap();
        assert_eq!(
            String::from_utf8_lossy(text.split_at(count as usize).0),
            "-12"
        );

        count = write_with_separator(&mut text.as_mut_slice(), "", '_', 2).unwrap();
        assert_eq!(String::from_utf8_lossy(text.split_at(count as usize).0), "");

        // degenerate but should not crash
        count = write_with_separator(&mut text.as_mut_slice(), "-", '_', 2).unwrap();
        assert_eq!(
            String::from_utf8_lossy(text.split_at(count as usize).0),
            "-"
        );
    }
}
