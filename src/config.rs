pub struct Config {
    pub show_decimal: bool,
    pub show_hexadecimal: bool,
    pub show_binary: bool,

    pub bit_width: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            show_decimal: true,
            show_hexadecimal: true,
            show_binary: true,
            bit_width: 32,
        }
    }
}
