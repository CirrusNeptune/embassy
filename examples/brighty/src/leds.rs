use defmt::{assert, info, debug};
use embassy_futures::select;
use embassy_rp::{dma, pio};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Instant, Timer};
use num_derive::FromPrimitive;
use crate::{consts, define_peripheral_set, Irqs};
use crate::sk6812::PioSK6812;
use crate::color::Color;

const LED_PERIOD: Duration = Duration::from_millis(20); // 50 Hz

pub const NUM_LEDS: usize = 10;


#[macro_export]
macro_rules! sk6812_peripherals {
    ($macro_name:ident $(,$arg:tt)*) => {
        $macro_name!{$($arg,)*
            SK6812Peripherals,
            dio: PIN_0,
            pio: PIO1,
            dma: DMA_CH1,
        }
    };
}

sk6812_peripherals!(define_peripheral_set);

#[derive(Copy, Clone)]
#[derive(FromPrimitive)]
pub enum Effect {
    Static = 0,
    Rainbow = 1,
}

#[derive(Copy, Clone)]
pub enum LedCommand {
    SetColorList([Color; NUM_LEDS]),
    ShiftColor(Color),
    SetPrimaryColor(Color),
    SetEffect(Effect),
    SetEffectSpeed(u16),
    SetBrightness(u8),
}

unsafe impl Send for LedCommand {}

pub type LedReceiver = Receiver<'static, CriticalSectionRawMutex, LedCommand, CHANNEL_BUF_LEN>;

pub struct LedSender(Sender<'static, CriticalSectionRawMutex, LedCommand, CHANNEL_BUF_LEN>);
impl LedSender {
    pub fn clone(&mut self) -> LedSender {
        LedSender(self.0.clone())
    }

    pub fn set_color_list(&mut self, color_list: [Color; NUM_LEDS]) {
        self.0.try_send(LedCommand::SetColorList(color_list)).ok();
    }

    pub fn shift_color(&mut self, color: Color) {
        self.0.try_send(LedCommand::ShiftColor(color)).ok();
    }

    pub fn set_primary_color(&mut self, color: Color) {
        self.0.try_send(LedCommand::SetPrimaryColor(color)).ok();
    }

    pub fn set_effect(&mut self, effect: Effect) {
        self.0.try_send(LedCommand::SetEffect(effect)).ok();
    }

    pub fn set_effect_speed(&mut self, effect_speed: u16) {
        self.0.try_send(LedCommand::SetEffectSpeed(effect_speed)).ok();
    }

    pub fn set_brightness(&mut self, brightness: u8) {
        self.0.try_send(LedCommand::SetBrightness(brightness)).ok();
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
        Self {
            keyframes: &DEFAULT_KEYFRAMES,
            last_frame: 0,
            frame_a: 0,
            frame_b: 0,
            ib: 1,
        }
    }
}

impl KeyframeReader {
    pub fn set_keyframes(&mut self, keyframes: &'static [Keyframe]) {
        self.keyframes = keyframes;

        self.last_frame = if let Some(kf) = keyframes.last() { kf.frame } else { 0 };

        self.frame_a = if let Some(kf) = keyframes.get(0) { kf.frame } else { 0 };

        self.frame_b = if let Some(kf) = keyframes.get(1) {
            kf.frame
        } else {
            self.frame_a
        };

        self.ib = 1;
    }

    pub fn evaluate_color_at_frame(&mut self, frame: u64) -> Color {
        if self.keyframes.is_empty() {
            return Color { r: 0, g: 0, b: 0, w: 0 };
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

        let ka = &self.keyframes[self.ib - 1];
        let kb = &self.keyframes[self.ib];
        let seg_duration = kb.frame - ka.frame;
        core::assert!(seg_duration > 0);
        let seg_instant = mod_frame - ka.frame;

        let r = (kb.color.r as u32 * seg_instant + ka.color.r as u32 * (seg_duration - seg_instant)) / seg_duration;
        let g = (kb.color.g as u32 * seg_instant + ka.color.g as u32 * (seg_duration - seg_instant)) / seg_duration;
        let b = (kb.color.b as u32 * seg_instant + ka.color.b as u32 * (seg_duration - seg_instant)) / seg_duration;
        let w = (kb.color.w as u32 * seg_instant + ka.color.w as u32 * (seg_duration - seg_instant)) / seg_duration;
        debug!("{} [{},{}]: ({} {} {} {})", mod_frame, self.ib - 1, self.ib, r, g, b, w);

        Color {
            r: r as u8,
            g: g as u8,
            b: b as u8,
            w: w as u8,
        }
    }
}

struct Leds<'d, PIO: pio::Instance, const SM: usize, DMA: dma::Channel> {
    sk6812: PioSK6812<'d, PIO, SM, DMA>,
    keyframe_readers: [KeyframeReader; NUM_LEDS],
    buffer: [u32; NUM_LEDS],
    primary_color: Color,
    effect: Effect,
    effect_speed: u16,
    brightness: u8,
}

const BRIGHTNESS_INTERP_MUL: u32 = 1;
const BRIGHTNESS_MAX: u32 = 31;
const BRIGHTNESS_MIN: u32 = 1;

impl<'d, PIO: pio::Instance, const SM: usize, DMA: dma::Channel> Leds<'d, PIO, SM, DMA> {
    pub fn new(sk6812: PioSK6812<'d, PIO, SM, DMA>) -> Self {
        let mut keyframe_readers: [KeyframeReader; NUM_LEDS] = [Default::default(); NUM_LEDS];

        Self {
            sk6812,
            keyframe_readers,
            buffer: [0; NUM_LEDS],
            primary_color: Color::BLACK,
            effect: Effect::Static,
            effect_speed: 32768,
            brightness: 255,
        }
    }

    pub async fn process_command(&mut self, cmd: &LedCommand) {
        match cmd {
            LedCommand::SetColorList(color_list) => {
                for (idx, color) in color_list.iter().enumerate() {
                    self.buffer[idx] = color.encode_for_sk6812();
                }
            }
            LedCommand::ShiftColor(color) => {
                for i in (1..NUM_LEDS).rev() {
                    self.buffer[i] = self.buffer[i-1];
                }
                self.buffer[0] = color.encode_for_sk6812();
            }
            LedCommand::SetPrimaryColor(color) => {
                self.primary_color = *color;
            }
            LedCommand::SetEffect(effect) => {
                self.effect = *effect;
            }
            LedCommand::SetEffectSpeed(effect_speed) => {
                self.effect_speed = *effect_speed;
            }
            LedCommand::SetBrightness(brightness) => {
                self.brightness = *brightness;
            }
        }
    }

    pub async fn tick(&mut self) {
        let cur_period = Instant::now().as_ticks() / LED_PERIOD.as_ticks();

        match self.effect {
            Effect::Static => {
                let encoded_color = self.primary_color.with_brightness(self.brightness).encode_for_sk6812();
                for i in 0..NUM_LEDS {
                    self.buffer[i] = encoded_color;
                }
            }
            Effect::Rainbow => {
                let base = ((cur_period * self.effect_speed as u64 / 64) % 65535) as u32;
                const LED_OFFSET: u32 = 65535_u32 / NUM_LEDS as u32;
                for i in 0..NUM_LEDS {
                    self.buffer[i] = Color::from_hsv(((base + LED_OFFSET * i as u32) % 65535) as u16, 255, self.brightness).encode_for_sk6812();
                }
            }
        }

        self.sk6812.write(&self.buffer).await;
    }

    pub async fn run(&mut self, receiver: LedReceiver) -> ! {
        loop {
            let next_tick = (Instant::now().as_ticks() + LED_PERIOD.as_ticks() - 1) / LED_PERIOD.as_ticks()
                * LED_PERIOD.as_ticks();
            match select::select(
                Timer::at(Instant::from_ticks(next_tick)),
                receiver.receive(),
            ).await {
                select::Either::First(_) => {
                    // Update timer has expired
                    self.tick().await;
                }
                select::Either::Second(command) => {
                    // Led command
                    self.process_command(&command).await;
                }
            }
        }
    }
}


#[embassy_executor::task]
pub async fn led_task(receiver: LedReceiver, p: SK6812Peripherals) -> ! {
    info!("set up SK6812 peripherals");
    let mut sk6812_pio = pio::Pio::new(p.pio, Irqs);
    let sk6812 = PioSK6812::new(
        &mut sk6812_pio.common,
        sk6812_pio.sm0,
        p.dio,
        p.dma,
    );
    Leds::new(sk6812).run(receiver).await
}
