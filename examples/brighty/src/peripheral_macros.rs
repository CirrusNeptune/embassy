#[macro_export]
macro_rules! define_peripheral_set {
    ($set_name:ident, $($name:ident: $type:ident,)*) => {
        pub struct $set_name {
            $(pub $name: embassy_rp::peripherals::$type,)*
        }
    };
}

#[macro_export]
macro_rules! take_peripheral_set {
    ($p:ident, $set_name:ident, $($name:ident: $type:ident,)*) => {
        $set_name {
            $($name: $p.$type,)*
        }
    };
}
