use defmt::{assert, info};
use embassy_futures::select;
use embassy_rp::{gpio, spi};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Instant, Timer};

use crate::command::{HaCommand, BUTTON_COMMANDS};
use crate::{consts, define_peripheral_set};

const LED_PERIOD: Duration = Duration::from_millis(20); // 50 Hz
const SLEEP_TIMEOUT_PERIOD: Duration = Duration::from_secs(30);

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
    SetButtonCheckedMask(u16),
    OrButtonCheckedMask(u16),
}

unsafe impl Send for LedCommand {}

pub type LedReceiver = Receiver<'static, CriticalSectionRawMutex, LedCommand, CHANNEL_BUF_LEN>;

pub struct LedSender(Sender<'static, CriticalSectionRawMutex, LedCommand, CHANNEL_BUF_LEN>);
impl LedSender {
    pub fn clone(&mut self) -> LedSender {
        LedSender(self.0.clone())
    }

    pub fn set_button_checked_mask(&mut self, mask: u16) {
        self.0.try_send(LedCommand::SetButtonCheckedMask(mask)).ok();
    }

    pub fn or_button_checked_mask(&mut self, mask: u16) {
        self.0.try_send(LedCommand::OrButtonCheckedMask(mask)).ok();
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

    pub fn on_turn_off(&mut self, entity_name: &str) {
        if entity_name != consts::DESK_STRIP_ENTITY {
            return;
        }

        self.set_button_checked_mask(0);
    }

    pub fn on_button_pressed(&mut self, i: usize) {
        self.or_button_checked_mask(1 << i);
    }
}

pub struct LedChannel(Channel<CriticalSectionRawMutex, LedCommand, CHANNEL_BUF_LEN>);

impl LedChannel {
    pub const fn new() -> Self {
        Self(Channel::new())
    }

    pub fn sender(&'static mut self) -> LedSender {
        LedSender(self.0.sender())
    }

    pub fn receiver(&'static mut self) -> LedReceiver {
        self.0.receiver()
    }
}

const CHANNEL_BUF_LEN: usize = 64;
pub(crate) static mut LED_CHANNEL: LedChannel = LedChannel::new();

struct SpiTx<'d, T: spi::Instance> {
    spi: spi::Spi<'d, T, spi::Async>,
    cs: gpio::Output<'d>,
}

impl<'d, T: spi::Instance> SpiTx<'d, T> {
    pub fn new(spi: spi::Spi<'d, T, spi::Async>, cs: gpio::Output<'d>) -> Self {
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

#[derive(Copy, Clone)]
struct KeyframeReader {
    keyframes: &'static [Keyframe],
    last_frame: u32,
    frame_a: u32,
    frame_b: u32,
    ib: usize,
}

impl Default for KeyframeReader {
    fn default() -> Self {
        static DEFAULT_KEYFRAMES: [Keyframe; 0] = [];
        Self { keyframes: &DEFAULT_KEYFRAMES, last_frame: 0, frame_a: 0, frame_b: 0, ib: 1 }
    }
}

impl KeyframeReader {
    pub fn set_keyframes(&mut self, keyframes: &'static [Keyframe]) {
        self.keyframes = keyframes;

        self.last_frame = if let Some(kf) = keyframes.last() {
            kf.frame
        } else {
            0
        };

        self.frame_a = if let Some(kf) = keyframes.get(0) {
            kf.frame
        } else {
            0
        };

        self.frame_b = if let Some(kf) = keyframes.get(1) {
            kf.frame
        } else {
            self.frame_a
        };

        self.ib = 1;
    }

    pub fn evaluate_color_at_frame(&mut self, frame: u64) -> Color {
        if self.keyframes.is_empty() {
            return Color { r: 0, g: 0, b: 0 };
        } else if self.keyframes.len() == 1 {
            return unsafe { self.keyframes.get_unchecked(0).color };
        }

        let mod_frame = (frame % self.last_frame as u64) as u32;
        if mod_frame < self.frame_a {
            self.ib = 1;
            self.frame_a = self.keyframes[self.ib - 1].frame;
            self.frame_b = self.keyframes[self.ib].frame;
        }
        if mod_frame >= self.frame_b {
            self.ib += 1;
            while self.keyframes[self.ib].frame < mod_frame {
                self.ib += 1;
            }
            self.frame_a = self.keyframes[self.ib - 1].frame;
            self.frame_b = self.keyframes[self.ib].frame;
        }

        let a = &self.keyframes[self.ib - 1];
        let b = &self.keyframes[self.ib];
        let seg_duration = b.frame - a.frame;
        assert!(seg_duration > 0);
        let seg_instant = mod_frame - a.frame;

        let r = (b.color.r as u32 * seg_instant + a.color.r as u32 * (seg_duration - seg_instant)) / seg_duration;
        let g = (b.color.g as u32 * seg_instant + a.color.g as u32 * (seg_duration - seg_instant)) / seg_duration;
        let b = (b.color.b as u32 * seg_instant + a.color.b as u32 * (seg_duration - seg_instant)) / seg_duration;
        //debug!("{} [{},{}]: ({} {} {})", mod_frame, self.ib - 1, self.ib, r, g, b);

        Color {
            r: r as u8,
            g: g as u8,
            b: b as u8,
        }
    }
}

struct Leds<'d, T: spi::Instance> {
    spi: SpiTx<'d, T>,
    keyframe_readers: [KeyframeReader; NUM_PADS],
    buffer: [u8; NUM_BUF_BYTES],
    checked_mask: u16,
    latch_mask: u16,
    brightness_buffer: [u32; NUM_PADS],
    last_period: u64,
    next_sleep_tick: Instant,
    sleep_pending: bool,
    sleeping: bool,
}

const BRIGHTNESS_INTERP_MUL: u32 = 1;
const BRIGHTNESS_MAX: u32 = 31;
const BRIGHTNESS_MIN: u32 = 1;

impl<'d, T: spi::Instance> Leds<'d, T> {
    pub fn new(spi: SpiTx<'d, T>) -> Self {
        let mut keyframe_readers: [KeyframeReader; NUM_PADS] = [Default::default(); NUM_PADS];
        let mut latch_mask = 0;
        for i in 0..NUM_PADS {
            if let Some(button_cmd) = BUTTON_COMMANDS.get(i) {
                keyframe_readers[i].set_keyframes(button_cmd.keyframes);
                latch_mask |= if button_cmd.command.led_latch() { 1 << i } else { 0 };
            }
        }

        Self {
            spi,
            keyframe_readers,
            buffer: [0_u8; NUM_BUF_BYTES],
            checked_mask: 0,
            latch_mask,
            brightness_buffer: [BRIGHTNESS_MAX * BRIGHTNESS_INTERP_MUL; NUM_PADS],
            last_period: 0,
            next_sleep_tick: Instant::MAX,
            sleep_pending: false,
            sleeping: false,
        }
    }

    pub async fn process_command(&mut self, cmd: &LedCommand) {
        match cmd {
            LedCommand::SetButtonCheckedMask(mask) => {
                self.checked_mask = *mask;
            }
            LedCommand::OrButtonCheckedMask(mask) => {
                self.checked_mask |= *mask;
                self.touch_sleep_timer();
            }
        }
    }

    pub fn set_led_value(&mut self, i: usize, brightness: u8, r: u8, g: u8, b: u8) {
        assert!(brightness <= 31);
        self.buffer[i * 4 + 4] = 0b11100000_u8 | brightness;
        self.buffer[i * 4 + 5] = b;
        self.buffer[i * 4 + 6] = g;
        self.buffer[i * 4 + 7] = r;
    }

    pub fn touch_sleep_timer(&mut self) {
        self.next_sleep_tick = Instant::now() + SLEEP_TIMEOUT_PERIOD;
        self.sleep_pending = false;
        self.sleeping = false;
    }

    pub async fn tick(&mut self) -> bool {
        let cur_period = Instant::now().as_ticks() / LED_PERIOD.as_ticks();
        let delta = if self.last_period != 0 {
            cur_period - self.last_period
        } else {
            0
        } as u32;
        self.last_period = cur_period;

        let mut all_brightness_bits = 0;
        for i in 0..NUM_PADS {
            let checked = ((1 << i) & self.checked_mask) != 0;
            let brightness_min = if self.sleep_pending { 0 } else { BRIGHTNESS_MIN };
            if checked && !self.sleep_pending {
                self.brightness_buffer[i] = BRIGHTNESS_MAX * BRIGHTNESS_INTERP_MUL;
            } else {
                self.brightness_buffer[i] = self.brightness_buffer[i]
                    .saturating_sub(delta)
                    .max(brightness_min * BRIGHTNESS_INTERP_MUL);
            }
            all_brightness_bits |= self.brightness_buffer[i];

            let color = self.keyframe_readers[i].evaluate_color_at_frame(cur_period * 10);
            self.set_led_value(
                i,
                (self.brightness_buffer[i] / BRIGHTNESS_INTERP_MUL) as u8,
                color.r,
                color.g,
                color.b,
            );
        }

        // Auto-clear according to latch mask after one update.
        self.checked_mask &= self.latch_mask;

        self.spi.send(&self.buffer).await;
        all_brightness_bits != 0
    }

    pub async fn run(&mut self, receiver: LedReceiver) -> ! {
        self.touch_sleep_timer();
        loop {
            if !self.sleeping {
                let next_tick =
                    (Instant::now().as_ticks() + LED_PERIOD.as_ticks() - 1) / LED_PERIOD.as_ticks() * LED_PERIOD.as_ticks();
                match select::select3(Timer::at(Instant::from_ticks(next_tick)), Timer::at(self.next_sleep_tick), receiver.receive()).await {
                    select::Either3::First(_) => {
                        // Update timer has expired
                        if !self.tick().await && self.sleep_pending {
                            self.sleeping = true;
                        }
                    }
                    select::Either3::Second(_) => {
                        // Sleep timer has expired
                        self.sleep_pending = true;
                    }
                    select::Either3::Third(command) => {
                        // Led command
                        self.process_command(&command).await;
                    }
                }
            } else {
                // Led command during sleep
                let command = receiver.receive().await;
                self.process_command(&command).await;
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
