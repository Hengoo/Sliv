pub struct Config {
    pub show_decimal: bool,
    pub show_hexadecimal: bool,
    pub show_binary: bool,

    pub bit_width: u32,
    // TODO bit map for what type of numbers we want to render
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
