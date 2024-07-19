use embassy_rp::dma::Channel;
use embassy_rp::gpio::{Drive, Level, SlewRate};
use embassy_rp::{Peripheral, PeripheralRef};
use embassy_rp::pio::{Common, Config, Direction, Instance, PioPin, ShiftDirection, StateMachine};
use embassy_rp::clocks::clk_sys_freq;
use pio_proc::pio_asm;
use fixed::{FixedU32, FixedU64};
use fixed::types::extra::U8;
use fixed::traits::FromFixed;

pub struct PioSK6812<'d, PIO: Instance, const SM: usize, DMA: Channel> {
    sm: StateMachine<'d, PIO, SM>,
    dma: PeripheralRef<'d, DMA>,
    wrap_target: u8,
}

impl<'d, PIO: Instance, const SM: usize, DMA: Channel> PioSK6812<'d, PIO, SM, DMA>
{
    /// Create a new instance of PioSK6812.
    pub fn new<DIO>(
        common: &mut Common<'d, PIO>,
        mut sm: StateMachine<'d, PIO, SM>,
        dio: DIO,
        dma: impl Peripheral<P = DMA> + 'd,
    ) -> Self
        where
            DIO: PioPin,
    {
        let program = pio_asm!(
            ".side_set 1"
            ".wrap_target"
            "bitloop:"
            "  out x, 1        side 0 [2]"
            "  jmp !x do_zero  side 1 [1]"
            "  jmp bitloop     side 1 [4]"
            "do_zero:"
            "  nop             side 0 [4]"
            ".wrap"
        );

        let mut pin_io = common.make_pio_pin(dio);
        pin_io.set_drive_strength(Drive::_12mA);
        pin_io.set_slew_rate(SlewRate::Fast);

        let mut cfg = Config::default();
        let loaded_program = common.load_program(&program.program);
        cfg.use_program(&loaded_program, &[&pin_io]);
        cfg.shift_out.direction = ShiftDirection::Left;
        cfg.shift_out.auto_fill = true;
        cfg.shift_out.threshold = 32;

        type Fix = FixedU32<U8>;
        type Fix64 = FixedU64<U8>;
        cfg.clock_divider = Fix::from_fixed(Fix64::from_num(clk_sys_freq()) / Fix64::const_from_int(8000000));

        sm.set_config(&cfg);

        sm.set_pin_dirs(Direction::Out, &[&pin_io]);
        sm.set_pins(Level::Low, &[&pin_io]);

        sm.set_enable(true);

        Self {
            sm,
            dma: dma.into_ref(),
            wrap_target: loaded_program.wrap.target,
        }
    }

    pub async fn write(&mut self, write: &[u32]) {
        self.sm.tx().dma_push(self.dma.reborrow(), write).await;
    }
}
