use crate::config::{self, Config};

const NUMBER_START_X: u32 = 8;
const NUMBER_START_Y: u32 = 2;

pub struct Layout {
    column_width: u32,
    table_height: u32,
}

impl Layout {
    pub fn new(config: Config) -> Layout {
        let unsigned_dec_width = 2u128.pow(config.bit_width).ilog10();
        Layout {
            column_width: 0,
            table_height: 0,
        }
    }
}
