use crate::{INumber, UNumber};
use anyhow::{Context, Ok, Result};
use core::str;
use std::io::Write;

// Computations and formating is done with [u8; NUMBER_STRING_WIDTH] on the stack
// Assumptions used in this file that are not validated:
//  - numbers only have leading spaces, but never trailing spaces
//  - strings only use u8 character

// We support 64 bit numbers formated as decimal or hex
// Binary designed for 16 bits -> need to split into multiple rows.

// technically we only need 26 but making it larger significantly simplifies suff
pub const NUMBER_STRING_WIDTH: usize = 32;

const CHAR_SPACE: u8 = ' ' as u8;
const CHAR_COMMA: u8 = ',' as u8;
const CHAR_MINUS: u8 = '-' as u8;

// Convert to negative interpretaton IF it aligns with one of
// the common integer sizes (i8, i16. i32, i64, i128)
fn handle_negative(number: UNumber) -> (bool, INumber) {
    if number == 0 {
        return (false, number as INumber);
    }

    let base = number.ilog2();
    if let 7 | 15 | 31 | 63 | 127 = base {
        let mut res = -1;
        res <<= base;
        res |= number as INumber;
        (true, res)
    } else {
        (false, number as INumber)
    }
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
    let block_count = (word_len + chars - 1) / chars;
    let block_len = chars + 1;
    let start_write = NUMBER_STRING_WIDTH
        .checked_sub(block_count * block_len)
        .context("Number is too long to add separator. Make sure to split binary numbers up")?;
    let start_read = NUMBER_STRING_WIDTH - block_count * chars;

    if block_count <= 1 {
        return Ok(());
    }

    // handle first block separately when it is only a minus
    let start_index = if text[start_read + chars - 1] == CHAR_MINUS {
        text[start_write + block_len] = CHAR_MINUS;
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
    let mut text = [CHAR_SPACE; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>0$b}", NUMBER_STRING_WIDTH)?;
    add_separator(&mut text, CHAR_SPACE, 4)?;
    Ok(text)
}

pub fn format_decimal(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let mut text = [CHAR_SPACE; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>0$}", NUMBER_STRING_WIDTH)?;
    add_separator(&mut text, CHAR_COMMA, 3)?;
    Ok(text)
}

pub fn format_signed_decimal(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let (_, number) = handle_negative(number);
    let mut text = [CHAR_SPACE; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>0$}", NUMBER_STRING_WIDTH)?;
    add_separator(&mut text, CHAR_COMMA, 3)?;
    Ok(text)
}

pub fn format_hexadecimal(number: UNumber) -> Result<[u8; NUMBER_STRING_WIDTH]> {
    let mut text = [CHAR_SPACE; NUMBER_STRING_WIDTH];
    write!(text.as_mut_slice(), "{number:>0$X}", NUMBER_STRING_WIDTH)?;
    add_separator(&mut text, CHAR_SPACE, 2)?;
    Ok(text)
}

fn remove_separator(
    mut text: [u8; NUMBER_STRING_WIDTH],
    separator: u8,
) -> [u8; NUMBER_STRING_WIDTH] {
    let word_len = text.len();
    let mut leading_spaces = 0;
    for i in 0..word_len {
        if text[i].is_ascii_whitespace() {
            leading_spaces = i;
        } else {
            break;
        }
    }

    let mut offset = leading_spaces;

    for i in leading_spaces..word_len {
        if text[i] == separator {
            text.copy_within(offset..i, offset + 1);
            text[offset] = CHAR_SPACE;
            offset += 1;
        }
    }
    text
}

pub fn parse_decimal(mut text: [u8; NUMBER_STRING_WIDTH]) -> Result<UNumber> {
    text = remove_separator(text, CHAR_COMMA);
    UNumber::from_str_radix(str::from_utf8(text.trim_ascii_start())?, 10)
        .context("parsing decimal number failed")
}

pub fn parse_signed_decimal(mut text: [u8; NUMBER_STRING_WIDTH]) -> Result<INumber> {
    text = remove_separator(text, CHAR_COMMA);
    INumber::from_str_radix(str::from_utf8(text.trim_ascii_start())?, 10)
        .context("parsing signed decimal number failed")
}

pub fn parse_hexadecimal(mut text: [u8; NUMBER_STRING_WIDTH]) -> Result<UNumber> {
    text = remove_separator(text, CHAR_SPACE);
    UNumber::from_str_radix(str::from_utf8(text.trim_ascii_start())?, 16)
        .context("parsing nexadecimal number failed")
}

pub fn parse_binary(mut text: [u8; NUMBER_STRING_WIDTH]) -> Result<UNumber> {
    text = remove_separator(text, CHAR_SPACE);
    UNumber::from_str_radix(str::from_utf8(text.trim_ascii_start())?, 2)
        .context("parsing binary number failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(text: &[u8]) -> [u8; NUMBER_STRING_WIDTH] {
        let mut result = [CHAR_SPACE; NUMBER_STRING_WIDTH];
        result
            .split_at_mut(NUMBER_STRING_WIDTH - text.len())
            .1
            .copy_from_slice(text);
        result
    }

    #[test]
    fn test_remove_separator() {
        assert_eq!(
            remove_separator(pad(b"test r t p b"), CHAR_SPACE).trim_ascii_start(),
            b"testrtpb"
        );

        assert_eq!(
            remove_separator(pad(b"1,234,567,890"), CHAR_SPACE).trim_ascii_start(),
            b"1,234,567,890"
        );
        assert_eq!(
            remove_separator(pad(b"-1,234,567,890"), CHAR_COMMA).trim_ascii_start(),
            b"-1234567890"
        );
        assert_eq!(
            remove_separator(b",,jjjjjj,j,,,-1,234,567,, ,,890,".clone(), CHAR_COMMA)
                .trim_ascii_start(),
            b"jjjjjjj-1234567 890"
        );
    }

    #[test]
    fn test_parse() {
        assert_eq!(parse_decimal(pad(b"123")).unwrap(), 123);
        assert_eq!(parse_decimal(pad(b"1,2,3")).unwrap(), 123);
        assert_eq!(parse_decimal(pad(b",,,123,,")).unwrap(), 123);
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
}
