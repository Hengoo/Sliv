use crate::{INumber, UNumber};
use anyhow::{Context, Ok, Result};
use std::io::Write;

// Computations and formating is done on the stack
// all strings are u8 arrays of length NUMBER_STRING_WIDTH
// formatter producer text with padding
// also has utility to remove padding and parse user change

// We support 64 bit numbers formated as decimal or hex
// Binary designed for 16 bits -> need to split into multiple rows.
// technically we only need 26 but making it larger significantly simplifies suff
const NUMBER_STRING_WIDTH: usize = 32;

const CHAR_SPACE: u8 = ' ' as u8;
const CHAR_COMMA: u8 = ',' as u8;
const CHAR_MINUS: u8 = '-' as u8;

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

fn add_separator(
    text: &mut [u8; NUMBER_STRING_WIDTH],
    separator: u8,
    char_between_separator: u32,
) -> Result<()> {
    // copy is done in place
    let chars = char_between_separator as usize;
    let len = text.len();
    let word_len = text.trim_ascii_start().len();
    // copy a few leading spaces, does not matter because we have padding
    let block_count = (word_len + chars - 1) / chars;
    let block_len = chars + 1;
    let start_write = len
        .checked_sub(block_count * block_len)
        .context("Number is too long to add separator. Make sure to split binary numbers up")?;
    let start_read = len - block_count * chars;

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

#[cfg(test)]
mod tests {
    use super::*;

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
