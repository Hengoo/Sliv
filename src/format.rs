use crate::{
    INumber, NUMBER_DIGIT_WIDTH, UNumber,
    column::{self, Cursor, Row},
};
use anyhow::{Context, Result, anyhow};
use core::str;
use std::{
    io::Write,
    ops::{Shl, Shr},
};

// Computations and formating is done with [u8; NUMBER_STRING_WIDTH] on the stack
// Assumptions used in this file that are not validated:
//  - numbers only have leading spaces, but never trailing spaces
//  - strings only use u8 character

// We support 64 bit numbers formated as decimal or hex
// Binary designed for 16 bits -> need to split into multiple rows.

// technically we only need 26 but making it larger significantly simplifies suff
pub const NUMBER_STRING_WIDTH: usize = 32;
pub const REAL_NUMBER_STRING_WIDTH: usize = NUMBER_DIGIT_WIDTH as usize;

pub const fn u8_char_to_number(char: u8) -> UNumber {
    #[allow(clippy::match_same_arms)]
    match char {
        b'0' => 0,
        b'1' => 1,
        b'2' => 2,
        b'3' => 3,
        b'4' => 4,
        b'5' => 5,
        b'6' => 6,
        b'7' => 7,
        b'8' => 8,
        b'9' => 9,
        b'a' | b'A' => 0xA,
        b'b' | b'B' => 0xB,
        b'c' | b'C' => 0xC,
        b'd' | b'D' => 0xD,
        b'e' | b'E' => 0xE,
        b'f' | b'F' => 0xF,
        _ => 0,
    }
}

// Translates one hex number (4bit) to a u8 char
pub const fn hex_to_u8_char(number: UNumber, offset: u32) -> u8 {
    match (number >> offset) & 0xF {
        0x0 => b'0',
        0x1 => b'1',
        0x2 => b'2',
        0x3 => b'3',
        0x4 => b'4',
        0x5 => b'5',
        0x6 => b'6',
        0x7 => b'7',
        0x8 => b'8',
        0x9 => b'9',
        0xA => b'A',
        0xB => b'B',
        0xC => b'C',
        0xD => b'D',
        0xE => b'E',
        0xF => b'F',
        _ => unreachable!(),
    }
}
const VALID_FLOAT_CHARACTERS: &[u8; 15] = b"0123456789.eE+-";

// Convert to negative interpretaton IF it aligns with one of
// the common integer sizes (i8, i16. i32, i64, i128)
pub const fn handle_negative(number: UNumber) -> INumber {
    if number == 0 {
        return number.cast_signed();
    }

    let base = number.ilog2();
    if let 7 | 15 | 31 | 63 | 127 = base {
        let mut res = -1;
        res <<= base;
        res |= number.cast_signed();
        res
    } else {
        number.cast_signed()
    }
}

// First tries to interpret as negative, if failed, convert to negative
pub const fn make_negative(number: UNumber) -> INumber {
    let mut number = handle_negative(number);
    if number.is_positive() {
        // this always works, because if fist bit is set (means this operation fails)
        // the number would already be negative from handle_negative
        number *= -1;
    }
    number
}
// adds separation character in number
// simplified example
// add_separator("12345", '#', 2) = "1#23#45"
// Actual usage would have to use u8 fixed size slice
// correctly supports negative numbers (minus is ignored)
fn add_separator(
    text: &mut [u8; NUMBER_STRING_WIDTH],
    separator: u8,
    char_between_separator: u32,
) -> Result<()> {
    // everything is done in place
    let chars = char_between_separator as usize;
    let word_len = text.trim_ascii_start().len();
    // might copy some leading spaces, does not matter because we always have adequate padding
    let block_count = word_len.div_ceil(chars);
    let block_len = chars + 1;
    let start_write = NUMBER_STRING_WIDTH
        .checked_sub(block_count * block_len)
        .context("Number is too long to add separator. Make sure to split binary numbers up")?;
    let start_read = NUMBER_STRING_WIDTH - block_count * chars;

    if block_count <= 1 {
        return Ok(());
    }

    // handle first block separately when it is only a minus
    let start_index = if text[start_read + chars - 1] == b'-' {
        text[start_write + block_len] = b'-';
        1
    } else {
        0
    };

    for i in start_index..block_count - 1 {
        text.copy_within(
            start_read + chars * i..start_read + chars * (i + 1),
            start_write + block_len * i + 1,
        );
        text[start_write + block_len * (i + 1)] = separator;
    }
    Ok(())
}

// writes a section (1/4) of the binary number
pub fn format_binary(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let mut text = [b' '; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>NUMBER_STRING_WIDTH$b}")?;
    add_separator(&mut text, b' ', 4)?;
    Ok(text)
}

pub fn format_decimal(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let mut text = [b' '; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>NUMBER_STRING_WIDTH$}")?;
    add_separator(&mut text, b',', 3)?;
    Ok(text)
}

pub fn format_signed_decimal(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let number = handle_negative(number);
    let mut text = [b' '; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>NUMBER_STRING_WIDTH$}")?;
    add_separator(&mut text, b',', 3)?;
    Ok(text)
}

pub fn format_hexadecimal(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let mut text = [b' '; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>NUMBER_STRING_WIDTH$X}")?;
    add_separator(&mut text, b' ', 2)?;
    Ok(text)
}

pub fn format_f64(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let mut text = [b' '; NUMBER_STRING_WIDTH];
    let double = f64::from_ne_bytes(number.to_ne_bytes());
    if write!(&mut text[6..], "{double:>REAL_NUMBER_STRING_WIDTH$}").is_err() {
        // Fallback to scientific if normal string formating does not fit
        // If https://stackoverflow.com/a/1701085 is correct, then we need at max 24 chars for this
        write!(&mut text[6..], "{double:>REAL_NUMBER_STRING_WIDTH$.e}")
                .context("Stackoverflow was wrong and the scientific representation of a double needs more than 26 chars")?;
    }
    Ok(text)
}

pub fn format_f32(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let mut text = [b' '; NUMBER_STRING_WIDTH];
    let float = f32::from_ne_bytes((number as u32).to_ne_bytes());
    if write!(&mut text[6..], "{float:>REAL_NUMBER_STRING_WIDTH$}").is_err() {
        // Fallback to scientific if normal string formating does not fit
        write!(&mut text[6..], "{float:>REAL_NUMBER_STRING_WIDTH$.e}")?;
    }
    Ok(text)
}

pub fn format_automatic(number: UNumber, row: Row) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    match row {
        Row::Decimal => format_decimal(number),
        Row::Signed => format_signed_decimal(number),
        Row::Hex => format_hexadecimal(number),
        Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3 => {
            // bin is split in 4 numbers to fit on screen
            let mask = UNumber::from(u16::MAX);
            let num_partial_row = 4 - (UNumber::BITS - number.leading_zeros()) as u8 / 16;
            let i = (row - 4) as u8;
            let mut text = if i >= num_partial_row {
                *b"             0000 0000 0000 0000"
            } else {
                *b"                                "
            };
            let num = (number >> ((3 - i) * 16)) & mask;
            if num != 0 || row == Row::Bin3 {
                let temp = format_binary(num)?;
                for (l, r) in text.iter_mut().zip(temp.iter()) {
                    if !r.is_ascii_whitespace() {
                        *l = *r;
                    }
                }
            }
            Ok(text)
        }
        Row::F64 => format_f64(number),
        Row::F32H => format_f32(number.shr(32)),
        Row::F32L => format_f32(u64::from(number as u32)),
        _ => Err(anyhow!("Wrong row")),
    }
}

fn parse_decimal(input: &str) -> Option<UNumber> {
    let mut input: String = input.trim().into();
    if input.starts_with("0x")
        || input.starts_with("0b")
        || input.starts_with("0o")
        || input.starts_with('-')
        || input.ends_with('d')
        || input.ends_with('f')
        || input.contains('.')
    {
        return None;
    }
    input.retain(|c| c.is_ascii_digit());
    UNumber::from_str_radix(&input, 10).ok()
}

fn parse_signed(input: &str) -> Option<UNumber> {
    let mut input: String = input.trim().into();
    if input.starts_with("0x")
        || input.starts_with("0b")
        || input.starts_with("0o")
        || input.ends_with('d')
        || input.ends_with('f')
        || input.contains('.')
    {
        return None;
    }
    input.retain(|c| c.is_ascii_digit() || c == '-');
    let signed = INumber::from_str_radix(&input, 10).ok();
    signed.map(INumber::cast_unsigned)
}

fn parse_hex(input: &str) -> Option<UNumber> {
    let mut input: String = input.trim().into();
    if input.starts_with("0b")
        || input.starts_with("0o")
        || input.starts_with('-')
        || input.ends_with('d')
        || input.ends_with('f')
        || input.contains('.')
    {
        return None;
    }
    input.retain(|c| c.is_ascii_hexdigit());
    UNumber::from_str_radix(&input, 16).ok()
}

fn parse_oct(input: &str) -> Option<UNumber> {
    let mut input: String = input.trim().into();
    if input.starts_with("0b")
        || input.starts_with("0x")
        || input.starts_with('-')
        || input.ends_with('d')
        || input.ends_with('f')
        || input.contains('.')
    {
        return None;
    }
    input.retain(|c| c.is_digit(8));
    UNumber::from_str_radix(&input, 8).ok()
}

fn parse_bin(input: &str) -> Option<UNumber> {
    let mut input: String = input.trim().into();
    if input.starts_with("0x")
        || input.starts_with("0o")
        || input.starts_with('-')
        || input.ends_with('d')
        || input.ends_with('f')
        || input.contains('.')
    {
        return None;
    }
    input.retain(|c| c.is_digit(2));
    UNumber::from_str_radix(&input, 2).ok()
}

fn parse_f64(mut input: &str) -> Option<UNumber> {
    input = input.trim();
    if input.starts_with("0b") || input.starts_with("0x") || input.starts_with("0o") {
        return None;
    }
    input = input.trim_end_matches('d');
    let mut input: String = input.into();
    input.retain(|c| c != ' ');
    if input.is_empty() {
        return Some(0);
    }
    input
        .parse::<f64>()
        .ok()
        .map(|f| UNumber::from_ne_bytes(f.to_ne_bytes()))
}

fn parse_f32(mut input: &str, row: Row) -> Option<UNumber> {
    input = input.trim();
    if input.starts_with("0b")
        || input.starts_with("0x")
        || input.starts_with("0o")
        || input.ends_with('d')
    {
        return None;
    }
    if !input.ends_with("inf") {
        input = input.trim_end_matches('f');
    }
    if !input.ends_with("INF") {
        input = input.trim_end_matches('F');
    }
    let mut input: String = input.into();
    input.retain(|c| c != ' ');
    if input.is_empty() {
        return Some(0);
    }
    match row {
        Row::F32H => input
            .parse::<f32>()
            .ok()
            .map(|f| UNumber::from(u32::from_ne_bytes(f.to_ne_bytes())).rotate_left(32)),
        Row::F32L => input
            .parse::<f32>()
            .ok()
            .map(|f| UNumber::from(u32::from_ne_bytes(f.to_ne_bytes()))),
        _ => unreachable!(),
    }
}

pub fn parse_user_input(input: &str, row: Row) -> Option<UNumber> {
    let input = input.trim();
    // first try parsing by type of cursor position
    let result = match row {
        Row::Decimal => parse_decimal(input),
        Row::Signed => parse_signed(input),
        Row::Hex => parse_hex(input),
        Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3 => parse_bin(input),
        Row::F64 => parse_f64(input),
        Row::F32H | Row::F32L => parse_f32(input, row),
        _ => None,
    };
    result
        .or_else(|| parse_decimal(input))
        .or_else(|| parse_signed(input))
        .or_else(|| parse_hex(input))
        .or_else(|| parse_bin(input))
        .or_else(|| parse_oct(input))
        .or_else(|| parse_f32(input, Row::F32L))
        .or_else(|| parse_f64(input))
}

pub fn replace_characters_automatic(number: UNumber, cursor: Cursor, chars: &[u8]) -> UNumber {
    let chars = chars.trim_ascii();
    match cursor.row {
        Row::Decimal => replace_chars_decimal(number, cursor, chars),
        Row::Signed => replace_chars_signed_decimal(number, cursor, chars),
        Row::Hex => replace_chars_hex(number, cursor, chars),
        Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3 => replace_chars_bin(number, cursor, chars),
        _ => number,
    }
}

fn replace_chars_decimal(number: UNumber, cursor: Cursor, chars: &[u8]) -> UNumber {
    // working with u128 to avoid unnecessary complexity doing this correctly
    let number: u128 = number.into();
    let mut middle: u128 = 0;
    let mut count = 0;
    let mut truncated_count = 0;
    for char in chars {
        match char {
            b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7' | b'8' | b'9' => {
                middle = middle.saturating_mul(10);
                middle = middle.saturating_add(u8_char_to_number(*char).into());
                count += 1;
                if middle != 0 {
                    truncated_count += 1;
                }
            }
            _ => {}
        }
    }
    let start_pos = column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize];
    if truncated_count + start_pos > 20 {
        return UNumber::MAX;
    }
    let end_pos = start_pos + count;

    let start_val = 10_u128.pow(start_pos.into());
    let end_val = 10_u128.pow(end_pos.into());

    let right = number % start_val;
    let left = number - number % end_val;
    middle *= 10_u128.pow(start_pos.into());
    let result = left + middle + right;
    result.try_into().unwrap_or(UNumber::MAX)
}

fn replace_chars_signed_decimal(number_input: UNumber, cursor: Cursor, chars: &[u8]) -> UNumber {
    // working with i128 to avoid unnecessary complexity doing this correctly
    let number: i128 = make_negative(number_input).into();
    let mut middle: i128 = 0;
    let mut count = 0;
    let mut truncated_count = 0;
    for char in chars {
        match char {
            b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7' | b'8' | b'9' => {
                middle = middle.saturating_mul(10);
                middle = middle.saturating_add(i128::from(u8_char_to_number(*char)));
                count += 1;
                if middle != 0 {
                    truncated_count += 1;
                }
            }
            _ => {}
        }
    }
    let start_pos = column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize];
    if truncated_count + start_pos > 20 {
        return INumber::MIN.cast_unsigned();
    }
    let end_pos = start_pos + count;

    let start_val = 10_i128.pow(start_pos.into());
    let end_val = 10_i128.pow(end_pos.into());

    let right = number % start_val;
    let left = number - number % end_val;
    middle *= -10_i128.pow(start_pos.into());

    let result = left + middle + right;
    if number == result {
        // avoid changing the other representations when the signed number stays the same
        number_input
    } else {
        INumber::try_from(result)
            .unwrap_or(INumber::MIN)
            .cast_unsigned()
    }
}

fn replace_chars_hex(mut number: UNumber, cursor: Cursor, chars: &[u8]) -> UNumber {
    let mut input_number: UNumber = 0;
    let mut bit_count = 0;
    for char in chars {
        match char {
            b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7' | b'8' | b'9' | b'A' | b'B'
            | b'C' | b'D' | b'E' | b'F' | b'a' | b'b' | b'c' | b'd' | b'e' | b'f' => {
                if let Some(num) = input_number.checked_shl(4) {
                    input_number = num;
                } else {
                    return UNumber::MAX;
                }
                input_number |= u8_char_to_number(*char);
                bit_count += 4;
            }
            _ => {}
        }
    }

    let bit_pos = column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize] * 4;
    let truncated_bit_count = UNumber::BITS - (input_number.leading_zeros() & !3);
    if truncated_bit_count + u32::from(bit_pos) > UNumber::BITS {
        return UNumber::MAX;
    }
    // let truncated_bit_count = UNumber::BITS - (input_number.leading_zeros() & !4);
    // if truncated_bit_count + u32::from(bit_pos) > UNumber::BITS {
    //     return UNumber::MAX;
    // }

    let mut mask = !(UNumber::MAX << bit_count);
    mask <<= bit_pos;
    input_number = input_number.shl(bit_pos);
    number &= !(mask);
    number |= input_number;
    number
}

fn replace_chars_bin(mut number: UNumber, cursor: Cursor, chars: &[u8]) -> UNumber {
    let mut input_number: UNumber = 0;
    let mut count = 0;
    for char in chars {
        match char {
            b'0' | b'1' => {
                input_number <<= 1;
                input_number |= u8_char_to_number(*char);
                count += 1;
            }
            _ => {}
        }
    }
    let bit_pos = column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize];

    let truncated_bit_count = UNumber::BITS - input_number.leading_zeros();
    if truncated_bit_count + u32::from(bit_pos) > UNumber::BITS {
        return UNumber::MAX;
    }

    let mut mask = !(UNumber::MAX << count);
    mask <<= bit_pos;
    input_number = input_number.shl(bit_pos);
    number &= !(mask);
    number |= input_number;
    number
}

// Shifts the caracters at the left of the cursor position in the requested direction
pub fn shift_characters_automatic(number_input: UNumber, cursor: Cursor, shift: i8) -> UNumber {
    if shift == 0 {
        return number_input;
    }
    match cursor.row {
        Row::Decimal => {
            let start_pos: u32 =
                column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize].into();
            if shift.is_positive() {
                let shift: u32 = shift.try_into().unwrap();
                let mut left = number_input - (number_input % 10_u64.pow(start_pos));
                let right = number_input % 10_u64.pow(start_pos);
                left = left.saturating_mul(10_u64.saturating_pow(shift));
                left.saturating_add(right)
            } else {
                let shift: u32 = (-shift).try_into().unwrap();
                let mut left = number_input - (number_input % 10_u64.saturating_pow(start_pos));
                let right = number_input % 10_u64.pow(start_pos.saturating_sub(shift));
                left /= 10_u64.saturating_pow(shift);
                left.saturating_add(right)
            }
        }
        Row::Signed => {
            let start_pos: u32 =
                column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize].into();
            let number = make_negative(number_input);
            if shift.is_positive() {
                let shift: u32 = shift.try_into().unwrap();
                let mut left = number - (number % 10_i64.saturating_pow(start_pos));
                let right = number % 10_i64.pow(start_pos);
                left = left.saturating_mul(10_i64.saturating_pow(shift));
                left.saturating_add(right).cast_unsigned()
            } else {
                let shift: u32 = (-shift).try_into().unwrap();
                let mut left = number - (number % 10_i64.saturating_pow(start_pos));
                let right = number % 10_i64.pow(start_pos.saturating_sub(shift));
                left /= 10_i64.saturating_pow(shift);
                left = left.saturating_add(right);
                if left == number {
                    // avoid changing the other representations when the signed number stays the same
                    number_input
                } else {
                    left.cast_unsigned()
                }
            }
        }
        Row::Hex | Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3 => {
            let char_size: u8 = if cursor.row == Row::Hex { 4 } else { 1 };
            let bit_shift: i16 = i16::from(shift) * i16::from(char_size);
            let bit_pos =
                column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize] * char_size;
            if shift.is_positive() {
                let bit_shift: u32 = bit_shift.try_into().unwrap();
                let left_mask = UNumber::MAX << bit_pos;
                let mut new_number =
                    (number_input & left_mask).saturating_mul(2_u64.saturating_pow(bit_shift));
                new_number |= number_input & !left_mask;
                new_number
            } else {
                let bit_shift: u32 = (-bit_shift).try_into().unwrap();
                if bit_shift >= UNumber::BITS {
                    return 0;
                }
                let left_mask = UNumber::MAX << bit_pos;
                let mut new_number = (number_input & left_mask).wrapping_shr(bit_shift);
                new_number |= number_input & (!left_mask).wrapping_shr(bit_shift);
                new_number
            }
        }
        _ => number_input,
    }
}

// valid characters that are actualy "numbers" (excluding '-')
pub fn is_valid_character_automatic(char: u8, row: Row) -> bool {
    match row {
        Row::Decimal | Row::Signed => char.is_ascii_digit(),
        Row::Hex => char.is_ascii_hexdigit(),
        Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3 => char == b'0' || char == b'1',
        Row::F64 | Row::F32H | Row::F32L => VALID_FLOAT_CHARACTERS.contains(&char),

        _ => false,
    }
}

// assumes that only valid characters are passed.
// Leading zero are also inserted
pub fn insert_characters_automatic(mut number: UNumber, cursor: Cursor, chars: &[u8]) -> UNumber {
    // combination of shift + replace
    number = shift_characters_automatic(number, cursor, chars.len().try_into().unwrap());
    match cursor.row {
        Row::Signed => {
            if number == INumber::MIN.cast_unsigned() {
                return number;
            }
        }
        _ => {
            if number == UNumber::MAX {
                return number;
            }
        }
    }
    replace_characters_automatic(number, cursor, chars)
}

pub fn remove_character_automatic(number: UNumber, cursor: Cursor) -> UNumber {
    let mut cursor_left = cursor;
    cursor_left.move_left();
    if cursor_left == cursor {
        // edge case left most character.
        // instead of shift we replace it with zero
        replace_characters_automatic(number, cursor, b"0")
    } else {
        shift_characters_automatic(number, cursor_left, -1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        assert_eq!(parse_decimal("123").unwrap(), 123);
        assert_eq!(parse_decimal("1,2,3").unwrap(), 123);
        assert_eq!(parse_decimal("0000,,,123,,").unwrap(), 123);
        assert_eq!(parse_decimal("1,234,567,890").unwrap(), 1_234_567_890);
        assert_eq!(
            parse_decimal("16,469,343,685,676,293,330").unwrap(),
            16_469_343_685_676_293_330
        );

        assert_eq!(parse_signed("123").unwrap(), 123);
        assert_eq!(parse_signed("1,2,3").unwrap(), 123);
        assert_eq!(parse_signed(",,,123,,").unwrap(), 123);
        assert_eq!(parse_signed("1,234,567,890").unwrap(), 1_234_567_890);
        assert_eq!(handle_negative(parse_signed("-123").unwrap()), -123);
        assert_eq!(handle_negative(parse_signed("-1,2,3").unwrap()), -123);
        assert_eq!(handle_negative(parse_signed(",,,-123,,").unwrap()), -123);
        assert_eq!(
            handle_negative(parse_signed("-1,234,567,890").unwrap()),
            -1_234_567_890
        );
        assert_eq!(
            handle_negative(parse_signed("-1,977,400,388,033,258,286").unwrap()),
            -1_977_400_388_033_258_286
        );

        assert_eq!(parse_hex("2A").unwrap(), 42);
        assert_eq!(parse_hex("7 5B CD 15").unwrap(), 123_456_789);
        assert_eq!(parse_hex("49 96 02 D2").unwrap(), 1_234_567_890);
        assert_eq!(
            parse_hex("E4 8E DC D2 E4 8E DC D2").unwrap(),
            16_469_343_685_676_293_330
        );
        assert_eq!(parse_hex("AFFEEE").unwrap(), 11_534_062);

        assert_eq!(parse_bin("1111").unwrap(), 15);
        assert_eq!(parse_bin("11111111").unwrap(), 255);
        assert_eq!(parse_bin("11  1 1  1 1 1 1").unwrap(), 255);
        assert_eq!(parse_bin("10 1010").unwrap(), 42);
        assert_eq!(parse_bin("1111 0010").unwrap(), 242);
        assert_eq!(parse_bin("1 0101 1110 0011 0110").unwrap(), 89654);

        assert_eq!(parse_f32("1.0", Row::F32L).unwrap(), 0x3f80_0000);
        assert_eq!(parse_f32("1", Row::F32L).unwrap(), 0x3f80_0000);
        assert_eq!(parse_f32("1.0f", Row::F32L).unwrap(), 0x3f80_0000);
        assert_eq!(parse_f32("1.0d", Row::F32L), None);
        assert_eq!(parse_f32("1.0", Row::F32H).unwrap(), 0x3f80_0000_0000_0000);

        assert_eq!(
            parse_f32("inf", Row::F32L).unwrap(),
            UNumber::from(u32::from_ne_bytes(f32::INFINITY.to_ne_bytes()))
        );
        assert_eq!(
            parse_f32("INF", Row::F32L).unwrap(),
            UNumber::from(u32::from_ne_bytes(f32::INFINITY.to_ne_bytes()))
        );
        assert_eq!(
            parse_f32("-inf", Row::F32L).unwrap(),
            UNumber::from(u32::from_ne_bytes(f32::NEG_INFINITY.to_ne_bytes()))
        );
        assert_eq!(
            parse_f32("nan", Row::F32L).unwrap(),
            UNumber::from(u32::from_ne_bytes(f32::NAN.to_ne_bytes()))
        );
        assert_eq!(
            parse_f32("NAN", Row::F32L).unwrap(),
            UNumber::from(u32::from_ne_bytes(f32::NAN.to_ne_bytes()))
        );
        assert_eq!(
            parse_f32("NaN", Row::F32L).unwrap(),
            UNumber::from(u32::from_ne_bytes(f32::NAN.to_ne_bytes()))
        );

        assert_eq!(parse_f64("1").unwrap(), 0x3FF0_0000_0000_0000);
        assert_eq!(parse_f64("1.0").unwrap(), 0x3FF0_0000_0000_0000);
        assert_eq!(parse_f64("1.0d").unwrap(), 0x3FF0_0000_0000_0000);
        assert_eq!(parse_f64("1.0f"), None);
        assert_eq!(
            parse_f64("inf").unwrap(),
            UNumber::from_ne_bytes(f64::INFINITY.to_ne_bytes())
        );
        assert_eq!(
            parse_f64("-inf").unwrap(),
            UNumber::from_ne_bytes(f64::NEG_INFINITY.to_ne_bytes())
        );
        assert_eq!(
            parse_f64("nan").unwrap(),
            UNumber::from_ne_bytes(f64::NAN.to_ne_bytes())
        );
    }

    fn prep(text: Result<[u8; NUMBER_STRING_WIDTH]>) -> String {
        let text = text.unwrap();
        String::from_utf8_lossy(text.trim_ascii_start()).into_owned()
    }

    #[test]
    fn test_format() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        let num = 42;
        assert_eq!(prep(format_binary(num)), "10 1010");
        assert_eq!(prep(format_decimal(num)), "42");
        assert_eq!(prep(format_signed_decimal(num)), "42");
        assert_eq!(prep(format_hexadecimal(num)), "2A");
        let num = 242;
        assert_eq!(prep(format_binary(num)), "1111 0010");
        assert_eq!(prep(format_decimal(num)), "242");
        assert_eq!(prep(format_signed_decimal(num)), "-14");
        assert_eq!(prep(format_hexadecimal(num)), "F2");
        let num = 123_456_789;
        // this and below binary numbers do not fit in string
        // assert_eq!(
        //     prep(format_binary(num)),
        //     "111 0101 1011 1100 1101 0001 0101"
        // );
        assert_eq!(prep(format_decimal(num)), "123,456,789");
        assert_eq!(prep(format_signed_decimal(num)), "123,456,789");
        assert_eq!(prep(format_hexadecimal(num)), "7 5B CD 15");
        let num = 1_234_567_890;
        // assert_eq!(
        //     prep(format_binary(num)),
        //     "100 1001 1001 0110 0000 0010 1101 0010"
        // );
        assert_eq!(prep(format_decimal(num)), "1,234,567,890");
        assert_eq!(prep(format_signed_decimal(num)), "1,234,567,890");
        assert_eq!(prep(format_hexadecimal(num)), "49 96 02 D2");
        let num = 3_834_567_890;
        // assert_eq!(
        //     prep(format_binary(num)),
        //     "1110 0100 1000 1110 1101 1100 1101 0010"
        // );
        assert_eq!(prep(format_decimal(num)), "3,834,567,890");
        assert_eq!(prep(format_signed_decimal(num)), "-460,399,406");
        assert_eq!(prep(format_hexadecimal(num)), "E4 8E DC D2");
        let num = 16_469_343_685_676_293_330;
        // assert_eq!(
        //     prep(format_binary(num)),
        //     "1110 0100 1000 1110 1101 1100 1101 0010 1110 0100 1000 1110 1101 1100 1101 0010"
        // );
        assert_eq!(prep(format_decimal(num)), "16,469,343,685,676,293,330");
        assert_eq!(
            prep(format_signed_decimal(num)),
            "-1,977,400,388,033,258,286"
        );
        assert_eq!(prep(format_hexadecimal(num)), "E4 8E DC D2 E4 8E DC D2");

        let num = 0x3f80_0000;
        assert_eq!(prep(format_f32(num)), "1");
        let num = 0x3ff0_0000_0000_0000;
        assert_eq!(prep(format_f64(num)), "1");
    }

    #[test]
    fn test_replace() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        let mut cursor = Cursor::default();
        // Unsigned
        let num = 51402;
        assert_eq!(replace_characters_automatic(num, cursor, b"00"), 51400);
        assert_eq!(replace_characters_automatic(num, cursor, b"00000"), 0);
        assert_eq!(
            replace_characters_automatic(num, cursor, b"2571640257"),
            2_571_640_257
        );
        assert_eq!(replace_characters_automatic(num, cursor, b"544"), 51544);
        cursor.move_left();
        cursor.move_left();
        cursor.move_left();
        assert_eq!(replace_characters_automatic(num, cursor, b"00"), 402);
        assert_eq!(replace_characters_automatic(num, cursor, b"137"), 137_402);
        cursor.text_pos = 1;
        assert_eq!(
            replace_characters_automatic(num, cursor, b"66"),
            UNumber::MAX
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"1"),
            10_000_000_000_000_051_402
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"01"),
            10_000_000_000_000_051_402
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"2"),
            UNumber::MAX
        );

        cursor = Cursor::default();
        cursor.move_down();
        // Signed
        let num: INumber = -51402;
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"00").cast_signed(),
            -51400
        );
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"00000").cast_signed(),
            -0
        );
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"2571640257").cast_signed(),
            -2_571_640_257
        );
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"544").cast_signed(),
            -51544
        );
        cursor.move_left();
        cursor.move_left();
        cursor.move_left();
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"00").cast_signed(),
            -402
        );
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"137").cast_signed(),
            -137_402
        );
        cursor.text_pos = 1;
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"66").cast_signed(),
            INumber::MIN
        );
        cursor.text_pos = 2;
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"1").cast_signed(),
            -1_000_000_000_000_051_402
        );
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"9").cast_signed(),
            -9_000_000_000_000_051_402
        );
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"09").cast_signed(),
            -9_000_000_000_000_051_402
        );
        assert_eq!(
            replace_characters_automatic(num.cast_unsigned(), cursor, b"10").cast_signed(),
            INumber::MIN
        );

        // Hex
        cursor = Cursor::default();
        cursor.move_down();
        cursor.move_down();
        let num: UNumber = 0xab_cdef;
        assert_eq!(
            replace_characters_automatic(num, cursor, b"00"),
            0x00ab_cd00
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"16fa"),
            0x00ab_16fa
        );
        cursor.move_left();
        assert_eq!(
            replace_characters_automatic(num, cursor, b"16fa"),
            0x00a1_6faf
        );
        cursor.move_left();
        cursor.move_left();
        cursor.move_left();
        assert_eq!(
            replace_characters_automatic(num, cursor, b"16fa"),
            0x16fa_cdef
        );
        cursor.text_pos = 5;
        assert_eq!(
            replace_characters_automatic(num, cursor, b"a"),
            0x0a00_0000_00ab_cdef
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"ba"),
            0xba00_0000_00ab_cdef
        );
        // Leading zeroes should not cause overflow
        assert_eq!(
            replace_characters_automatic(num, cursor, b"0ba"),
            0xba00_0000_00ab_cdef
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"1ba"),
            UNumber::MAX
        );
        cursor.text_pos = 4;
        assert_eq!(
            replace_characters_automatic(num, cursor, b"3"),
            0x3000_0000_00ab_cdef
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"F"),
            0xf000_0000_00ab_cdef
        );

        // Bin
        cursor = Cursor::default();
        cursor.move_down();
        cursor.move_down();
        cursor.move_down();

        cursor.move_down();
        cursor.move_down();
        cursor.move_down();
        let num: UNumber = 0b100_1100;
        assert_eq!(replace_characters_automatic(num, cursor, b"11"), 0b100_1111);
        assert_eq!(
            replace_characters_automatic(num, cursor, b"0000"),
            0b100_0000
        );
        cursor.move_left();
        cursor.move_left();
        cursor.move_left();
        assert_eq!(replace_characters_automatic(num, cursor, b"11"), 0b101_1100);
        cursor.move_up();
        cursor.move_up();
        cursor.move_up();
        cursor.text_pos = 9;
        assert_eq!(
            replace_characters_automatic(num, cursor, b"11"),
            0b1100_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0100_1100
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"111"),
            UNumber::MAX
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"011"),
            0b1100_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0100_1100
        );
    }

    #[test]
    fn test_shift() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        // Unsigned
        let mut cursor = Cursor::default();
        let mut num = 42;
        assert_eq!(shift_characters_automatic(num, cursor, 1), 420);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 4);
        assert_eq!(shift_characters_automatic(num, cursor, 3), 42000);
        cursor.move_left();
        assert_eq!(shift_characters_automatic(num, cursor, 3), 40002);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 4);
        num = 6_402_155_412;
        cursor.move_left();
        assert_eq!(shift_characters_automatic(num, cursor, 0), 6_402_155_412);
        assert_eq!(shift_characters_automatic(num, cursor, 1), 64_021_554_012);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 640_215_542);
        assert_eq!(shift_characters_automatic(num, cursor, -2), 64_021_554);
        assert_eq!(shift_characters_automatic(num, cursor, -3), 6_402_155);
        assert_eq!(shift_characters_automatic(num, cursor, -9), 6);
        assert_eq!(shift_characters_automatic(num, cursor, -10), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -11), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -21), 0);
        assert_eq!(shift_characters_automatic(num, cursor, 11), UNumber::MAX);
        assert_eq!(shift_characters_automatic(num, cursor, 21), UNumber::MAX);

        // Signed
        cursor = Cursor::default();
        cursor.move_down();
        let mut num = (-42_i64).cast_unsigned();
        assert_eq!(
            shift_characters_automatic(num, cursor, 1).cast_signed(),
            -420
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, -1).cast_signed(),
            -4
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, 3).cast_signed(),
            -42000
        );
        cursor.move_left();
        assert_eq!(
            shift_characters_automatic(num, cursor, 3).cast_signed(),
            -40002i64
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, -1).cast_signed(),
            -4_i64
        );
        num = (-6_402_155_412i64).cast_unsigned();
        cursor.move_left();
        assert_eq!(
            shift_characters_automatic(num, cursor, 0).cast_signed(),
            -6_402_155_412
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, 1).cast_signed(),
            -64_021_554_012
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, -1).cast_signed(),
            -640_215_542
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, -2).cast_signed(),
            -64_021_554
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, -3).cast_signed(),
            -6_402_155
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, -9).cast_signed(),
            -6
        );
        assert_eq!(shift_characters_automatic(num, cursor, -10), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -11), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -21), 0);
        assert_eq!(
            shift_characters_automatic(num, cursor, 11),
            INumber::MIN.cast_unsigned()
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, 21),
            INumber::MIN.cast_unsigned()
        );

        // Hex
        cursor = Cursor::default();
        cursor.move_down();
        cursor.move_down();
        num = 0x00ab_cdef;
        assert_eq!(shift_characters_automatic(num, cursor, 1), 0x0abc_def0);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 0xabcde);
        assert_eq!(shift_characters_automatic(num, cursor, 3), 0x000a_bcde_f000);
        cursor.move_left();
        assert_eq!(shift_characters_automatic(num, cursor, 1), 0x0abc_de0f);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 0xabcde);
        cursor.move_left();
        assert_eq!(shift_characters_automatic(num, cursor, 1), 0x0abc_d0ef);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 0xabcdf);
        assert_eq!(
            shift_characters_automatic(num, cursor, 10),
            0xabcd_0000_0000_00ef
        );
        assert_eq!(shift_characters_automatic(num, cursor, 11), UNumber::MAX);
        assert_eq!(shift_characters_automatic(num, cursor, 21), UNumber::MAX);
        assert_eq!(shift_characters_automatic(num, cursor, -11), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -8), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -20), 0);

        // Bin
        cursor = Cursor::default();
        cursor.move_down();
        cursor.move_down();
        cursor.move_down();

        cursor.move_down();
        cursor.move_down();
        cursor.move_down();
        num = 0b100_0100;
        assert_eq!(shift_characters_automatic(num, cursor, 1), 0b1000_1000);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 0b10_0010);
        assert_eq!(shift_characters_automatic(num, cursor, 3), 0b10_0010_0000);
        cursor.move_left();
        cursor.move_left();
        cursor.move_left();
        assert_eq!(shift_characters_automatic(num, cursor, 1), 0b1000_0100);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 0b10_0000);
        assert_eq!(
            shift_characters_automatic(num, cursor, 10),
            0b1_0000_0000_0000_0100
        );
        assert_eq!(shift_characters_automatic(num, cursor, 61), UNumber::MAX);
        assert_eq!(shift_characters_automatic(num, cursor, 111), UNumber::MAX);
        assert_eq!(shift_characters_automatic(num, cursor, -11), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -40), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -100), 0);
    }
}
