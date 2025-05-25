use crate::{
    column::{self, Cursor, Row},
    INumber, UNumber,
};
use anyhow::{anyhow, Context, Result};
use core::str;
use std::{io::Write, ops::Shl};

// Computations and formating is done with [u8; NUMBER_STRING_WIDTH] on the stack
// Assumptions used in this file that are not validated:
//  - numbers only have leading spaces, but never trailing spaces
//  - strings only use u8 character

// We support 64 bit numbers formated as decimal or hex
// Binary designed for 16 bits -> need to split into multiple rows.

// technically we only need 26 but making it larger significantly simplifies suff
pub const NUMBER_STRING_WIDTH: usize = 32;

#[allow(dead_code)]
pub fn char_to_number(char: char) -> UNumber {
    match char {
        '0' => 0,
        '1' => 1,
        '2' => 2,
        '3' => 3,
        '4' => 4,
        '5' => 5,
        '6' => 6,
        '7' => 7,
        '8' => 8,
        '9' => 9,
        'a' | 'A' => 0xA,
        'b' | 'B' => 0xB,
        'c' | 'C' => 0xC,
        'd' | 'D' => 0xD,
        'e' | 'E' => 0xE,
        'f' | 'F' => 0xF,
        _ => 0,
    }
}

pub fn u8_char_to_number(char: u8) -> UNumber {
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
pub fn hex_to_u8_char(number: UNumber, offset: u32) -> u8 {
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

// Convert to negative interpretaton IF it aligns with one of
// the common integer sizes (i8, i16. i32, i64, i128)
pub fn handle_negative(number: UNumber) -> INumber {
    if number == 0 {
        return number as INumber;
    }

    let base = number.ilog2();
    if let 7 | 15 | 31 | 63 | 127 = base {
        let mut res = -1;
        res <<= base;
        res |= number as INumber;
        res
    } else {
        number as INumber
    }
}

// First tries to interpret as negative, if failed, convert to negative
pub fn make_negative(number: UNumber) -> INumber {
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

pub fn format_binary(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let mut text = [b' '; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>0$b}", NUMBER_STRING_WIDTH)?;
    add_separator(&mut text, b' ', 4)?;
    Ok(text)
}

pub fn format_decimal(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let mut text = [b' '; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>0$}", NUMBER_STRING_WIDTH)?;
    add_separator(&mut text, b',', 3)?;
    Ok(text)
}

pub fn format_signed_decimal(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let number = handle_negative(number);
    if number.is_positive() {
        // TODO change format to return result Opion
        // return None
    }
    let mut text = [b' '; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>0$}", NUMBER_STRING_WIDTH)?;
    add_separator(&mut text, b',', 3)?;
    Ok(text)
}

pub fn format_hexadecimal(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let mut text = [b' '; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>0$X}", NUMBER_STRING_WIDTH)?;
    add_separator(&mut text, b' ', 2)?;
    Ok(text)
}

fn remove_separator(
    mut text: [u8; NUMBER_STRING_WIDTH],
    separator: u8,
) -> [u8; NUMBER_STRING_WIDTH] {
    let word_len = text.len();
    let mut leading_spaces = 0;

    for (i, char) in text.iter().enumerate() {
        if char.is_ascii_whitespace() {
            leading_spaces = i
        } else {
            break;
        }
    }

    let mut offset = leading_spaces;

    for i in leading_spaces..word_len {
        if text[i] == separator {
            text.copy_within(offset..i, offset + 1);
            text[offset] = b' ';
            offset += 1;
        }
    }
    text
}

pub fn parse_decimal(mut text: [u8; NUMBER_STRING_WIDTH]) -> Result<UNumber> {
    text = remove_separator(text, b',');
    let res = UNumber::from_str_radix(str::from_utf8(text.trim_ascii_start())?, 10);
    if let Ok(res) = res {
        return Ok(res);
    }
    let err = res.unwrap_err();
    match err.kind() {
        std::num::IntErrorKind::PosOverflow => Ok(UNumber::MAX),
        std::num::IntErrorKind::NegOverflow => Ok(UNumber::MIN),
        _ => Err(anyhow!(err).context("Parsing unsigned decimal number failed")),
    }
}

pub fn parse_signed_decimal(mut text: [u8; NUMBER_STRING_WIDTH]) -> Result<INumber> {
    text = remove_separator(text, b',');
    let res = INumber::from_str_radix(str::from_utf8(text.trim_ascii_start())?, 10);
    if let Ok(res) = res {
        return Ok(res);
    }
    let err = res.unwrap_err();
    match err.kind() {
        std::num::IntErrorKind::PosOverflow => Ok(INumber::MAX),
        std::num::IntErrorKind::NegOverflow => Ok(INumber::MIN),
        _ => Err(anyhow!(err).context("Parsing signed decimal number failed")),
    }
}

pub fn parse_hexadecimal(mut text: [u8; NUMBER_STRING_WIDTH]) -> Result<UNumber> {
    text = remove_separator(text, b' ');
    let res = UNumber::from_str_radix(str::from_utf8(text.trim_ascii_start())?, 16);
    if let Ok(res) = res {
        return Ok(res);
    }
    let err = res.unwrap_err();
    match err.kind() {
        std::num::IntErrorKind::PosOverflow => Ok(UNumber::MAX),
        std::num::IntErrorKind::NegOverflow => Ok(UNumber::MIN),
        _ => Err(anyhow!(err).context("Parsing nexadecimal number failed")),
    }
}

pub fn parse_binary(mut text: [u8; NUMBER_STRING_WIDTH]) -> Result<UNumber> {
    text = remove_separator(text, b' ');
    let res = UNumber::from_str_radix(str::from_utf8(text.trim_ascii_start())?, 2);
    if let Ok(res) = res {
        return Ok(res);
    }
    let err = res.unwrap_err();
    match err.kind() {
        std::num::IntErrorKind::PosOverflow => Ok(UNumber::MAX),
        std::num::IntErrorKind::NegOverflow => Ok(UNumber::MIN),
        _ => Err(anyhow!(err).context("Parsing binary number failed")),
    }
}

pub fn combine_number_text(left: &mut [u8; NUMBER_STRING_WIDTH], right: [u8; NUMBER_STRING_WIDTH]) {
    let mut is_neg = false;
    for (l, r) in left.iter_mut().zip(right.iter()) {
        if *r == b'-' {
            is_neg = true;
        } else if !r.is_ascii_whitespace() {
            *l = *r;
        }
    }
    // moves minus to leftmost char to avoid the user writing numbers left of it
    if is_neg {
        left[0] = b'-';
    }
}

pub fn format_automatic(number: UNumber, row: Row) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    match row {
        Row::Decimal => format_decimal(number),
        Row::Signed => format_signed_decimal(number),
        Row::Hex => format_hexadecimal(number),
        Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3 => {
            // bin is split in 4 numbers to fit on screen
            let mask = u16::MAX as UNumber;
            let num_partial_row = 4 - (UNumber::BITS - number.leading_zeros()) as u8 / 16;
            let i = (row - 4) as u8;
            let mut text = if i >= num_partial_row {
                *b"             0000 0000 0000 0000"
            } else {
                *b"                                "
            };
            let num = (number >> ((3 - i) * 16)) & mask;
            if num != 0 || row == Row::Bin3 {
                combine_number_text(&mut text, format_binary(num)?);
            }
            Ok(text)
        }
        _ => Err(anyhow!("Wrong row?")),
    }
}

pub fn parse_automatic(text: [u8; NUMBER_STRING_WIDTH], row: Row) -> Result<UNumber> {
    match row {
        Row::Decimal => parse_decimal(text),
        Row::Signed => parse_signed_decimal(text).map(|n| n as UNumber),
        Row::Hex => parse_hexadecimal(text),
        Row::Bin0 | Row::Bin1 | Row::Bin2 | Row::Bin3 => parse_binary(text),
        _ => Err(anyhow!("Wrong row?")),
    }
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
    // working with u128 to avoid absurd complexity doing this correctly
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

fn replace_chars_signed_decimal(number: UNumber, cursor: Cursor, chars: &[u8]) -> UNumber {
    // working with i128 to avoid absurd complexity doing this correctly
    let number: i128 = make_negative(number).into();
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
        return INumber::MIN as UNumber;
    }
    let end_pos = start_pos + count;

    let start_val = 10_i128.pow(start_pos.into());
    let end_val = 10_i128.pow(end_pos.into());

    let right = number % start_val;
    let left = number - number % end_val;
    middle *= -10_i128.pow(start_pos.into());

    let result = left + middle + right;
    INumber::try_from(result).unwrap_or(INumber::MIN) as UNumber
}

fn replace_chars_hex(mut number: UNumber, cursor: Cursor, chars: &[u8]) -> UNumber {
    let mut input_number: UNumber = 0;
    let mut bit_count = 0;
    for char in chars {
        match char {
            b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7' | b'8' | b'9' | b'A' | b'B'
            | b'C' | b'D' | b'E' | b'F' | b'a' | b'b' | b'c' | b'd' | b'e' | b'f' => {
                input_number <<= 4;
                input_number |= u8_char_to_number(*char);
                bit_count += 4;
            }
            _ => {}
        }
    }

    let bit_pos = column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize] * 4;
    let truncated_bit_count = UNumber::BITS - (input_number.leading_zeros() & !4);
    if truncated_bit_count + u32::from(bit_pos) > UNumber::BITS {
        return UNumber::MAX;
    }

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

pub fn shift_characters_automatic(number: UNumber, cursor: Cursor, shift: i8) -> UNumber {
    if shift == 0 {
        return number;
    }
    match cursor.row {
        Row::Decimal => {
            let start_pos: u32 =
                column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize].into();
            if shift.is_positive() {
                let shift: u32 = shift.try_into().unwrap();
                let mut left = number - (number % 10_u64.pow(start_pos));
                let right = number % 10_u64.pow(start_pos);
                left = left.saturating_mul(10_u64.saturating_pow(shift));
                left.saturating_add(right)
            } else {
                let shift: u32 = (-shift).try_into().unwrap();
                let mut left = number - (number % 10_u64.saturating_pow(start_pos));
                let right = number % 10_u64.pow(start_pos.saturating_sub(shift));
                left /= 10_u64.saturating_pow(shift);
                left.saturating_add(right)
            }
        }
        Row::Signed => {
            let start_pos: u32 =
                column::LOOKUP_TABLE[cursor.row as usize][cursor.text_pos as usize].into();
            let number = make_negative(number);
            if shift.is_positive() {
                let shift: u32 = shift.try_into().unwrap();
                let mut left = number - (number % 10_i64.saturating_pow(start_pos));
                let right = number % 10_i64.pow(start_pos);
                left = left.saturating_mul(10_i64.saturating_pow(shift));
                left.saturating_add(right) as UNumber
            } else {
                let shift: u32 = (-shift).try_into().unwrap();
                let mut left = number - (number % 10_i64.saturating_pow(start_pos));
                let right = number % 10_i64.pow(start_pos.saturating_sub(shift));
                left /= 10_i64.saturating_pow(shift);
                left.saturating_add(right) as UNumber
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
                    (number & left_mask).saturating_mul(2_u64.saturating_pow(bit_shift));
                new_number |= number & !left_mask;
                new_number
            } else {
                let bit_shift: u32 = (-bit_shift).try_into().unwrap();
                if bit_shift >= UNumber::BITS {
                    return 0;
                }
                let left_mask = UNumber::MAX << bit_pos;
                let mut new_number = (number & left_mask).wrapping_shr(bit_shift);
                new_number |= number & (!left_mask).wrapping_shr(bit_shift);
                new_number
            }
        }
        _ => number,
    }
}

pub fn insert_characters_automatic(mut number: UNumber, cursor: Cursor, chars: &[u8]) -> UNumber {
    // combination of shift + replace
    if chars.len() > 1 {
        // need to compute actual length beforehand
        todo!()
    }
    number = shift_characters_automatic(number, cursor, 1);
    match cursor.row {
        Row::Signed => {
            if number == INumber::MIN as UNumber {
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

    fn pad(text: &[u8]) -> [u8; NUMBER_STRING_WIDTH] {
        let mut result = [b' '; NUMBER_STRING_WIDTH];
        result
            .split_at_mut(NUMBER_STRING_WIDTH - text.len())
            .1
            .copy_from_slice(text);
        result
    }

    #[test]
    fn test_remove_separator() {
        std::env::set_var("RUST_BACKTRACE", "1");
        assert_eq!(
            remove_separator(pad(b"test r t p b"), b' ').trim_ascii_start(),
            b"testrtpb"
        );

        assert_eq!(
            remove_separator(pad(b"1,234,567,890"), b' ').trim_ascii_start(),
            b"1,234,567,890"
        );
        assert_eq!(
            remove_separator(pad(b"-1,234,567,890"), b',').trim_ascii_start(),
            b"-1234567890"
        );
        assert_eq!(
            remove_separator(*b",,jjjjjj,j,,,-1,234,567,, ,,890,", b',').trim_ascii_start(),
            b"jjjjjjj-1234567 890"
        );
    }

    #[test]
    fn test_parse() {
        std::env::set_var("RUST_BACKTRACE", "1");
        assert_eq!(parse_decimal(pad(b"123")).unwrap(), 123);
        assert_eq!(parse_decimal(pad(b"1,2,3")).unwrap(), 123);
        assert_eq!(parse_decimal(pad(b"0000,,,123,,")).unwrap(), 123);
        assert_eq!(parse_decimal(pad(b"1,234,567,890")).unwrap(), 1234567890);
        assert_eq!(
            parse_decimal(pad(b"16,469,343,685,676,293,330")).unwrap(),
            16469343685676293330
        );

        assert_eq!(parse_signed_decimal(pad(b"123")).unwrap(), 123);
        assert_eq!(parse_signed_decimal(pad(b"1,2,3")).unwrap(), 123);
        assert_eq!(parse_signed_decimal(pad(b",,,123,,")).unwrap(), 123);
        assert_eq!(
            parse_signed_decimal(pad(b"1,234,567,890")).unwrap(),
            1234567890
        );
        assert_eq!(parse_signed_decimal(pad(b"-123")).unwrap(), -123);
        assert_eq!(parse_signed_decimal(pad(b"-1,2,3")).unwrap(), -123);
        assert_eq!(parse_signed_decimal(pad(b",,,-123,,")).unwrap(), -123);
        assert_eq!(
            parse_signed_decimal(pad(b"-1,234,567,890")).unwrap(),
            -1234567890
        );
        assert_eq!(
            parse_signed_decimal(pad(b"-1,977,400,388,033,258,286")).unwrap(),
            -1977400388033258286
        );

        assert_eq!(parse_hexadecimal(pad(b"2A")).unwrap(), 42);
        assert_eq!(parse_hexadecimal(pad(b"7 5B CD 15")).unwrap(), 123456789);
        assert_eq!(parse_hexadecimal(pad(b"49 96 02 D2")).unwrap(), 1234567890);
        assert_eq!(
            parse_hexadecimal(pad(b"E4 8E DC D2 E4 8E DC D2")).unwrap(),
            16469343685676293330
        );
        assert_eq!(parse_hexadecimal(pad(b"AFFEEE")).unwrap(), 11534062);

        assert_eq!(parse_binary(pad(b"1111")).unwrap(), 15);
        assert_eq!(parse_binary(pad(b"11111111")).unwrap(), 255);
        assert_eq!(parse_binary(pad(b"11  1 1  1 1 1 1")).unwrap(), 255);
        assert_eq!(parse_binary(pad(b"10 1010")).unwrap(), 42);
        assert_eq!(parse_binary(pad(b"1111 0010")).unwrap(), 242);
        assert_eq!(parse_binary(pad(b"1 0101 1110 0011 0110")).unwrap(), 89654);
    }

    fn prep(text: Result<[u8; NUMBER_STRING_WIDTH]>) -> String {
        let text = text.unwrap();
        String::from_utf8_lossy(text.trim_ascii_start()).into_owned()
    }

    #[test]
    fn test_format() {
        std::env::set_var("RUST_BACKTRACE", "1");
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
        let num = 123456789;
        // this and below binary numbers do not fit in string
        // assert_eq!(
        //     prep(format_binary(num)),
        //     "111 0101 1011 1100 1101 0001 0101"
        // );
        assert_eq!(prep(format_decimal(num)), "123,456,789");
        assert_eq!(prep(format_signed_decimal(num)), "123,456,789");
        assert_eq!(prep(format_hexadecimal(num)), "7 5B CD 15");
        let num = 1234567890;
        // assert_eq!(
        //     prep(format_binary(num)),
        //     "100 1001 1001 0110 0000 0010 1101 0010"
        // );
        assert_eq!(prep(format_decimal(num)), "1,234,567,890");
        assert_eq!(prep(format_signed_decimal(num)), "1,234,567,890");
        assert_eq!(prep(format_hexadecimal(num)), "49 96 02 D2");
        let num = 3834567890;
        // assert_eq!(
        //     prep(format_binary(num)),
        //     "1110 0100 1000 1110 1101 1100 1101 0010"
        // );
        assert_eq!(prep(format_decimal(num)), "3,834,567,890");
        assert_eq!(prep(format_signed_decimal(num)), "-460,399,406");
        assert_eq!(prep(format_hexadecimal(num)), "E4 8E DC D2");
        let num = 16469343685676293330;
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
    }

    #[test]
    fn test_replace() {
        std::env::set_var("RUST_BACKTRACE", "1");
        let mut cursor = Cursor::default();
        // Unsigned
        let num = 51402;
        assert_eq!(replace_characters_automatic(num, cursor, b"00"), 51400);
        assert_eq!(replace_characters_automatic(num, cursor, b"00000"), 0);
        assert_eq!(
            replace_characters_automatic(num, cursor, b"2571640257"),
            2571640257
        );
        assert_eq!(replace_characters_automatic(num, cursor, b"544"), 51544);
        cursor.move_left();
        cursor.move_left();
        cursor.move_left();
        assert_eq!(replace_characters_automatic(num, cursor, b"00"), 402);
        assert_eq!(replace_characters_automatic(num, cursor, b"137"), 137402);
        cursor.text_pos = 1;
        assert_eq!(
            replace_characters_automatic(num, cursor, b"66"),
            UNumber::MAX
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"1"),
            10000000000000051402
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"01"),
            10000000000000051402
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
            replace_characters_automatic(num as UNumber, cursor, b"00") as INumber,
            -51400
        );
        assert_eq!(
            replace_characters_automatic(num as UNumber, cursor, b"00000") as INumber,
            -0
        );
        assert_eq!(
            replace_characters_automatic(num as UNumber, cursor, b"2571640257") as INumber,
            -2571640257
        );
        assert_eq!(
            replace_characters_automatic(num as UNumber, cursor, b"544") as INumber,
            -51544
        );
        cursor.move_left();
        cursor.move_left();
        cursor.move_left();
        assert_eq!(
            replace_characters_automatic(num as UNumber, cursor, b"00") as INumber,
            -402
        );
        assert_eq!(
            replace_characters_automatic(num as UNumber, cursor, b"137") as INumber,
            -137402
        );
        cursor.text_pos = 1;
        assert_eq!(
            replace_characters_automatic(num as UNumber, cursor, b"66") as INumber,
            INumber::MIN
        );
        cursor.text_pos = 2;
        assert_eq!(
            replace_characters_automatic(num as UNumber, cursor, b"1") as INumber,
            -1000000000000051402
        );
        assert_eq!(
            replace_characters_automatic(num as UNumber, cursor, b"9") as INumber,
            -9000000000000051402
        );
        assert_eq!(
            replace_characters_automatic(num as UNumber, cursor, b"09") as INumber,
            -9000000000000051402
        );
        assert_eq!(
            replace_characters_automatic(num as UNumber, cursor, b"10") as INumber,
            INumber::MIN
        );

        // Hex
        cursor = Default::default();
        cursor.move_down();
        cursor.move_down();
        let num: UNumber = 0xabcdef;
        assert_eq!(replace_characters_automatic(num, cursor, b"00"), 0xabcd00);
        assert_eq!(replace_characters_automatic(num, cursor, b"16fa"), 0xab16fa);
        cursor.move_left();
        assert_eq!(replace_characters_automatic(num, cursor, b"16fa"), 0xa16faf);
        cursor.move_left();
        cursor.move_left();
        cursor.move_left();
        assert_eq!(
            replace_characters_automatic(num, cursor, b"16fa"),
            0x16facdef
        );
        cursor.text_pos = 5;
        assert_eq!(
            replace_characters_automatic(num, cursor, b"a"),
            0xa00000000abcdef
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"ba"),
            0xba00000000abcdef
        );
        // Leading zeroes should not cause overflow
        assert_eq!(
            replace_characters_automatic(num, cursor, b"0ba"),
            0xba00000000abcdef
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"1ba"),
            UNumber::MAX
        );

        // Bin
        cursor = Default::default();
        cursor.move_down();
        cursor.move_down();
        cursor.move_down();

        cursor.move_down();
        cursor.move_down();
        cursor.move_down();
        let num: UNumber = 0b1001100;
        assert_eq!(replace_characters_automatic(num, cursor, b"11"), 0b1001111);
        assert_eq!(
            replace_characters_automatic(num, cursor, b"0000"),
            0b1000000
        );
        cursor.move_left();
        cursor.move_left();
        cursor.move_left();
        assert_eq!(replace_characters_automatic(num, cursor, b"11"), 0b1011100);
        cursor.move_up();
        cursor.move_up();
        cursor.move_up();
        cursor.text_pos = 9;
        assert_eq!(
            replace_characters_automatic(num, cursor, b"11"),
            0b1100000000000000000000000000000000000000000000000000000001001100
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"111"),
            UNumber::MAX
        );
        assert_eq!(
            replace_characters_automatic(num, cursor, b"011"),
            0b1100000000000000000000000000000000000000000000000000000001001100
        );
    }

    #[test]
    fn test_shift() {
        std::env::set_var("RUST_BACKTRACE", "1");
        // Unsigned
        let mut cursor = Cursor::default();
        let mut num = 42;
        assert_eq!(shift_characters_automatic(num, cursor, 1), 420);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 4);
        assert_eq!(shift_characters_automatic(num, cursor, 3), 42000);
        cursor.move_left();
        assert_eq!(shift_characters_automatic(num, cursor, 3), 40002);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 4);
        num = 6402155412;
        cursor.move_left();
        assert_eq!(shift_characters_automatic(num, cursor, 0), 6402155412);
        assert_eq!(shift_characters_automatic(num, cursor, 1), 64021554012);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 640215542);
        assert_eq!(shift_characters_automatic(num, cursor, -2), 64021554);
        assert_eq!(shift_characters_automatic(num, cursor, -3), 6402155);
        assert_eq!(shift_characters_automatic(num, cursor, -9), 6);
        assert_eq!(shift_characters_automatic(num, cursor, -10), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -11), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -21), 0);
        assert_eq!(shift_characters_automatic(num, cursor, 11), UNumber::MAX);
        assert_eq!(shift_characters_automatic(num, cursor, 21), UNumber::MAX);

        // Signed
        cursor = Cursor::default();
        cursor.move_down();
        let mut num = (-42_i64) as UNumber;
        assert_eq!(shift_characters_automatic(num, cursor, 1) as INumber, -420);
        assert_eq!(shift_characters_automatic(num, cursor, -1) as INumber, -4);
        assert_eq!(
            shift_characters_automatic(num, cursor, 3) as INumber,
            -42000
        );
        cursor.move_left();
        assert_eq!(
            shift_characters_automatic(num, cursor, 3) as INumber,
            -40002_i64
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, -1) as INumber,
            -4_i64
        );
        num = (-6402155412_i64) as UNumber;
        cursor.move_left();
        assert_eq!(
            shift_characters_automatic(num, cursor, 0) as INumber,
            -6402155412
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, 1) as INumber,
            -64021554012
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, -1) as INumber,
            -640215542
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, -2) as INumber,
            -64021554
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, -3) as INumber,
            -6402155
        );
        assert_eq!(shift_characters_automatic(num, cursor, -9) as INumber, -6);
        assert_eq!(shift_characters_automatic(num, cursor, -10), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -11), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -21), 0);
        assert_eq!(
            shift_characters_automatic(num, cursor, 11),
            INumber::MIN as UNumber
        );
        assert_eq!(
            shift_characters_automatic(num, cursor, 21),
            INumber::MIN as UNumber
        );

        // Hex
        cursor = Cursor::default();
        cursor.move_down();
        cursor.move_down();
        num = 0xabcdef;
        assert_eq!(shift_characters_automatic(num, cursor, 1), 0xabcdef0);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 0xabcde);
        assert_eq!(shift_characters_automatic(num, cursor, 3), 0xabcdef000);
        cursor.move_left();
        assert_eq!(shift_characters_automatic(num, cursor, 1), 0xabcde0f);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 0xabcde);
        cursor.move_left();
        assert_eq!(shift_characters_automatic(num, cursor, 1), 0xabcd0ef);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 0xabcdf);
        assert_eq!(
            shift_characters_automatic(num, cursor, 10),
            0xabcd0000000000ef
        );
        assert_eq!(shift_characters_automatic(num, cursor, 11), UNumber::MAX);
        assert_eq!(shift_characters_automatic(num, cursor, 21), UNumber::MAX);
        assert_eq!(shift_characters_automatic(num, cursor, -11), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -8), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -20), 0);

        // Bin
        cursor = Default::default();
        cursor.move_down();
        cursor.move_down();
        cursor.move_down();

        cursor.move_down();
        cursor.move_down();
        cursor.move_down();
        num = 0b1000100;
        assert_eq!(shift_characters_automatic(num, cursor, 1), 0b10001000);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 0b100010);
        assert_eq!(shift_characters_automatic(num, cursor, 3), 0b1000100000);
        cursor.move_left();
        cursor.move_left();
        cursor.move_left();
        assert_eq!(shift_characters_automatic(num, cursor, 1), 0b10000100);
        assert_eq!(shift_characters_automatic(num, cursor, -1), 0b100000);
        assert_eq!(
            shift_characters_automatic(num, cursor, 10),
            0b10000000000000100
        );
        assert_eq!(shift_characters_automatic(num, cursor, 61), UNumber::MAX);
        assert_eq!(shift_characters_automatic(num, cursor, 111), UNumber::MAX);
        assert_eq!(shift_characters_automatic(num, cursor, -11), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -40), 0);
        assert_eq!(shift_characters_automatic(num, cursor, -100), 0);
    }
}
