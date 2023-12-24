#![allow(dead_code)]

pub const ADDR: u8 = 0x20; // default addr

macro_rules! tcaregs {
    ($($name:ident : $val:expr),* $(,)?) => {
        $(
            pub const $name: u8 = $val;
        )*

        pub fn regname(reg: u8) -> &'static str {
            match reg {
                $(
                    $val => stringify!($name),
                )*
                _ => panic!("bad reg"),
            }
        }
    }
}

tcaregs! {
    INPORT0: 0x00,
    INPORT1: 0x01,
    OUTPORT0: 0x02,
    OUTPORT1: 0x03,
    POLINV0: 0x04,
    POLINV1: 0x05,
    CONF0: 0x06,
    CONF1: 0x07,
}
