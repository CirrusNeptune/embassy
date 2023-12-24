use defmt::{assert, info};
use embassy_futures::select;
use embassy_rp::{gpio, spi};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::zerocopy_channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Instant, Timer};
use static_cell::StaticCell;

use crate::command::{HaCommand, BUTTON_COMMANDS};
use crate::{consts, define_peripheral_set};

const LED_PERIOD: Duration = Duration::from_millis(20); // 50 Hz

#[macro_export]
macro_rules! led_peripherals {
    ($macro_name:ident $(,$arg:tt)*) => {
        $macro_name!{$($arg,)*
            LedPeripherals,
            mosi: PIN_19,
            clk: PIN_18,
            cs: PIN_17,
            dma1: DMA_CH1,
            spi0: SPI0,
        }
    };
}

led_peripherals!(define_peripheral_set);

#[derive(Copy, Clone)]
pub enum LedCommand {
    DisconnectState,
    ConnectState,
    SetButtonCheckedMask(u16),
    OrButtonCheckedMask(u16),
}

pub type LedReceiver = Receiver<'static, ThreadModeRawMutex, Option<LedCommand>>;

pub struct LedSender(Sender<'static, ThreadModeRawMutex, Option<LedCommand>>);
impl LedSender {
    pub fn borrow(&mut self) -> LedSender {
        // SAFETY: inner channel reference is 'static
        LedSender(unsafe { &mut *(self as *mut LedSender) }.0.borrow())
    }

    pub fn set_button_checked_mask(&mut self, mask: u16) {
        if let Some(v) = self.0.try_send() {
            v.replace(LedCommand::SetButtonCheckedMask(mask));
            self.0.send_done();
        }
    }

    pub fn or_button_checked_mask(&mut self, mask: u16) {
        if let Some(v) = self.0.try_send() {
            v.replace(LedCommand::OrButtonCheckedMask(mask));
            self.0.send_done();
        }
    }

    pub fn on_effect_changed(&mut self, entity_name: &str, effect_name: &str) {
        if entity_name != consts::DESK_STRIP_ENTITY {
            return;
        }

        if let Some(button_idx) = BUTTON_COMMANDS.iter().position(|cmd| match cmd.command {
            HaCommand::SetEffect(effect) => {
                return effect.effect_name == effect_name;
            }
            _ => false,
        }) {
            self.set_button_checked_mask(1 << button_idx);
        } else {
            self.set_button_checked_mask(0);
        }
    }

    pub fn on_button_pressed(&mut self, i: usize) {
        self.or_button_checked_mask(1 << i);
    }
}

pub struct LedChannel(Channel<'static, ThreadModeRawMutex, Option<LedCommand>>);

impl LedChannel {
    pub fn new(buf: &'static mut [Option<LedCommand>]) -> Self {
        Self(Channel::new(buf))
    }
    pub fn split(&'static mut self) -> (LedSender, LedReceiver) {
        let (sender, receiver) = self.0.split();
        (LedSender(sender), receiver)
    }
}

const CHANNEL_BUF_LEN: usize = 64;

pub fn make_channel() -> &'static mut LedChannel {
    static BUF: StaticCell<[Option<LedCommand>; CHANNEL_BUF_LEN]> = StaticCell::new();
    let buf = BUF.init([Default::default(); CHANNEL_BUF_LEN]);
    static CHANNEL: StaticCell<LedChannel> = StaticCell::new();
    CHANNEL.init(LedChannel::new(buf))
}

struct SpiTx<'d, T: spi::Instance, P: gpio::Pin> {
    spi: spi::Spi<'d, T, spi::Async>,
    cs: gpio::Output<'d, P>,
}

impl<'d, T: spi::Instance, P: gpio::Pin> SpiTx<'d, T, P> {
    pub fn new(spi: spi::Spi<'d, T, spi::Async>, cs: gpio::Output<'d, P>) -> Self {
        Self { spi, cs }
    }

    pub async fn send(&mut self, buffer: &[u8]) {
        self.cs.set_low();
        self.spi.write(&buffer).await.unwrap();
        self.cs.set_high();
    }
}

const WIDTH: usize = 4;
const HEIGHT: usize = 4;
const NUM_PADS: usize = WIDTH * HEIGHT;
const NUM_BUF_BYTES: usize = (NUM_PADS * 4) + 8;

#[derive(Copy, Clone)]
pub struct Color {
    pub(crate) r: u8,
    pub(crate) g: u8,
    pub(crate) b: u8,
}

#[derive(Copy, Clone)]
pub struct Keyframe {
    pub(crate) frame: u32,
    pub(crate) color: Color,
}

struct KeyframeReader {
    keyframes: &'static [Keyframe],
    cur_keyframe: u32,
}

fn evaluate_color_at_frame(keyframes: &[Keyframe], frame: u64) -> Color {
    if keyframes.is_empty() {
        return Color { r: 0, g: 0, b: 0 };
    } else if keyframes.len() == 1 {
        return unsafe { keyframes.get_unchecked(0).color };
    }
    let last_frame = unsafe { keyframes.get_unchecked(keyframes.len() - 1).frame };
    let mod_frame = (frame % last_frame as u64) as u32;
    let mut ib = 0;
    for (i, kf) in keyframes.iter().enumerate() {
        if kf.frame > mod_frame {
            ib = i;
            break;
        }
    }
    assert!(ib > 0);
    let a = &keyframes[ib - 1];
    let b = &keyframes[ib];
    let seg_duration = b.frame - a.frame;
    assert!(seg_duration > 0);
    let seg_instant = mod_frame - a.frame;
    let r = (b.color.r as u32 * seg_instant + a.color.r as u32 * (seg_duration - seg_instant)) / seg_duration;
    let g = (b.color.g as u32 * seg_instant + a.color.g as u32 * (seg_duration - seg_instant)) / seg_duration;
    let b = (b.color.b as u32 * seg_instant + a.color.b as u32 * (seg_duration - seg_instant)) / seg_duration;
    //debug!("{} [{},{}]: {} {} {}", mod_frame, ib - 1, ib, r, g, b);
    Color {
        r: r as u8,
        g: g as u8,
        b: b as u8,
    }
}

struct Leds<'d, T: spi::Instance, P: gpio::Pin> {
    spi: SpiTx<'d, T, P>,
    buffer: [u8; NUM_BUF_BYTES],
    checked_mask: u16,
    brightness_buffer: [u32; NUM_PADS],
    last_period: u64,
}

const BRIGHTNESS_INTERP_MUL: u32 = 1;
const BRIGHTNESS_MAX: u32 = 31;
const BRIGHTNESS_MIN: u32 = 1;

impl<'d, T: spi::Instance, P: gpio::Pin> Leds<'d, T, P> {
    pub fn new(spi: SpiTx<'d, T, P>) -> Self {
        Self {
            spi,
            buffer: [0_u8; NUM_BUF_BYTES],
            checked_mask: 0,
            brightness_buffer: [BRIGHTNESS_MAX * BRIGHTNESS_INTERP_MUL; NUM_PADS],
            last_period: 0,
        }
    }

    pub async fn process_command(&mut self, cmd: &LedCommand) {
        match cmd {
            LedCommand::SetButtonCheckedMask(mask) => {
                self.checked_mask = *mask;
            }
            LedCommand::OrButtonCheckedMask(mask) => {
                self.checked_mask |= *mask;
            }
            _ => {}
        }
    }

    pub fn set_led_value(&mut self, i: usize, brightness: u8, r: u8, g: u8, b: u8) {
        assert!(brightness <= 31);
        self.buffer[i * 4 + 4] = 0b11100000_u8 | brightness;
        self.buffer[i * 4 + 5] = b;
        self.buffer[i * 4 + 6] = g;
        self.buffer[i * 4 + 7] = r;
    }

    pub async fn tick(&mut self) {
        let cur_period = Instant::now().as_ticks() / LED_PERIOD.as_ticks();
        let delta = if self.last_period != 0 {
            cur_period - self.last_period
        } else {
            0
        } as u32;
        self.last_period = cur_period;

        for i in 0..NUM_PADS {
            let checked = ((1 << i) & self.checked_mask) != 0;
            if checked {
                self.brightness_buffer[i] = BRIGHTNESS_MAX * BRIGHTNESS_INTERP_MUL;
            } else {
                self.brightness_buffer[i] = self.brightness_buffer[i]
                    .saturating_sub(delta)
                    .max(BRIGHTNESS_MIN * BRIGHTNESS_INTERP_MUL);
            }

            if let Some(button_cmd) = BUTTON_COMMANDS.get(i) {
                let color = evaluate_color_at_frame(button_cmd.keyframes, cur_period * 10);
                self.set_led_value(
                    i,
                    (self.brightness_buffer[i] / BRIGHTNESS_INTERP_MUL) as u8,
                    color.r,
                    color.g,
                    color.b,
                );
            } else {
                self.set_led_value(i, 0, 0, 0, 0);
            }
        }
        self.spi.send(&self.buffer).await;
    }

    pub async fn run(&mut self, mut receiver: LedReceiver) -> ! {
        loop {
            let next_tick =
                (Instant::now().as_ticks() + LED_PERIOD.as_ticks() - 1) / LED_PERIOD.as_ticks() * LED_PERIOD.as_ticks();
            match select::select(Timer::at(Instant::from_ticks(next_tick)), receiver.receive()).await {
                select::Either::First(_) => {
                    // Update timer has expired
                    self.tick().await;
                }
                select::Either::Second(command) => {
                    // Led command
                    let owned_command = *command;
                    receiver.receive_done();
                    if let Some(cmd) = owned_command {
                        self.process_command(&cmd).await;
                    }
                }
            }
        }
    }
}

#[embassy_executor::task]
pub async fn led_task(receiver: LedReceiver, p: LedPeripherals) -> ! {
    info!("set up leds");
    let spi_config = spi::Config::new(
        4 * 1024 * 1024,
        spi::Phase::CaptureOnFirstTransition,
        spi::Polarity::IdleLow,
    );
    let spi = spi::Spi::new_txonly(p.spi0, p.clk, p.mosi, p.dma1, spi_config);
    let cs = gpio::Output::new(p.cs, gpio::Level::High);
    Leds::new(SpiTx::new(spi, cs)).run(receiver).await
}
