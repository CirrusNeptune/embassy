use defmt::{info, unwrap};
use embassy_rp::{gpio, i2c};

use crate::command::CommandSender;
use crate::leds::LedSender;
use crate::{define_peripheral_set, tca9555, Irqs};

#[macro_export]
macro_rules! button_peripherals {
    ($macro_name:ident $(,$arg:tt)*) => {
        $macro_name!{$($arg,)*
            ButtonPeripherals,
            sda: PIN_4,
            scl: PIN_5,
            i2c0: I2C0,
            button_int: PIN_3,
        }
    };
}

button_peripherals!(define_peripheral_set);

struct Buttons<'d, T: i2c::Instance> {
    i2c: i2c::I2c<'d, T, i2c::Async>,
    button_int: gpio::Input<'d>,
    sender: CommandSender,
    led_sender: LedSender,
}

impl<'d, T: i2c::Instance> Buttons<'d, T> {
    pub fn new(
        i2c: i2c::I2c<'d, T, i2c::Async>,
        button_int: gpio::Input<'d>,
        sender: CommandSender,
        led_sender: LedSender,
    ) -> Self {
        Self {
            i2c,
            button_int,
            sender,
            led_sender,
        }
    }

    pub async fn read_buttons(&mut self) -> u16 {
        use embedded_hal_async::i2c::I2c;
        let mut port0 = [0; 2];
        unwrap!(
            self.i2c
                .write_read(tca9555::ADDR, &[tca9555::INPORT0], &mut port0)
                .await
        );
        u16::from_le_bytes(port0)
    }

    fn on_button_pressed(&mut self, i: usize) {
        info!("button {} pressed", i);
        self.sender.on_button_pressed(i);
        self.led_sender.on_button_pressed(i);
    }

    fn on_button_released(&mut self, i: usize) {
        info!("button {} released", i);
    }

    pub async fn run(&mut self) -> ! {
        let mut states = self.read_buttons().await;
        loop {
            self.button_int.wait_for_low().await;
            let new_states = self.read_buttons().await;
            let flips = states ^ new_states;

            if flips != 0 {
                for i in 0..16 {
                    if (flips >> i) & 0x1 != 0 {
                        if (new_states >> i) & 0x1 != 0 {
                            self.on_button_released(i);
                        } else {
                            self.on_button_pressed(i);
                        }
                    }
                }
            }

            states = new_states;
        }
    }
}

#[embassy_executor::task]
pub async fn button_task(sender: CommandSender, led_sender: LedSender, p: ButtonPeripherals) -> ! {
    info!("set up i2c");
    let i2c = i2c::I2c::new_async(p.i2c0, p.scl, p.sda, Irqs, i2c::Config::with_frequency(400_000));
    let button_int = gpio::Input::new(p.button_int, gpio::Pull::None);
    Buttons::new(i2c, button_int, sender, led_sender).run().await
}
