#![no_std]
#![no_main]

mod buttons;
mod command;
mod consts;
mod leds;
mod peripheral_macros;
mod tca9555;
mod websocket;

use crate::leds::LedSender;
use buttons::{button_task, ButtonPeripherals};
use consts::HA_CONSTS;
use cyw43_pio::PioSpi;
use defmt::{debug, info, unwrap};
use embassy_executor::{Executor, Spawner};
use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Config, DhcpConfig, IpEndpoint, Stack, StackResources};
use embassy_rp::gpio::{Level, Output};
use embassy_rp::multicore;
use embassy_rp::multicore::spawn_core1;
use embassy_rp::peripherals::{DMA_CH0, I2C0, PIN_23, PIN_25, PIO0};
use embassy_rp::{bind_interrupts, i2c, pio};
use embassy_time::Timer;
use leds::{led_task, LedPeripherals};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

const WIFI_NETWORK: &str = "JAMzzz";
const WIFI_PASSWORD: &str = include_str!("../wifi_password.txt");

bind_interrupts!(struct Irqs {
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

#[embassy_executor::task]
async fn wifi_task(
    runner: cyw43::Runner<'static, Output<'static, PIN_23>, PioSpi<'static, PIN_25, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<cyw43::NetDriver<'static>>) -> ! {
    stack.run().await
}

macro_rules! wifi_peripherals {
    ($macro_name:ident $(,$arg:tt)*) => {
        $macro_name!{$($arg,)*
            WifiPeripherals,
            pwr: PIN_23,
            cs: PIN_25,
            pio: PIO0,
            dio: PIN_24,
            clk: PIN_29,
            dma0: DMA_CH0,
        }
    };
}

wifi_peripherals!(define_peripheral_set);

#[embassy_executor::task]
async fn core0_task(
    spawner: Spawner,
    wifi_peripherals: WifiPeripherals,
    button_peripherals: ButtonPeripherals,
    mut led_sender: LedSender,
) {
    let fw = include_bytes!("../../../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../../../cyw43-firmware/43439A0_clm.bin");

    info!("set up wifi peripherals");
    let pwr = Output::new(wifi_peripherals.pwr, Level::Low);
    let cs = Output::new(wifi_peripherals.cs, Level::High);
    let mut pio = pio::Pio::new(wifi_peripherals.pio, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        pio.irq0,
        cs,
        wifi_peripherals.dio,
        wifi_peripherals.clk,
        wifi_peripherals.dma0,
    );

    info!("set up cyw43");
    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
    unwrap!(spawner.spawn(wifi_task(runner)));

    info!("init cyw43");
    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    let mut dhcp_config: DhcpConfig = Default::default();
    dhcp_config.hostname = Some(unwrap!("squishy".try_into()));
    let config = Config::dhcpv4(dhcp_config);

    // Generate random seed
    let seed = {
        use rand_core::RngCore;
        embassy_rp::clocks::RoscRng.next_u64()
    };
    debug!("rand seed {}", seed);

    // Init network stack
    static STACK: StaticCell<Stack<cyw43::NetDriver<'static>>> = StaticCell::new();
    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let stack = &*STACK.init(Stack::new(
        net_device,
        config,
        RESOURCES.init(StackResources::<3>::new()),
        seed,
    ));

    info!("set up net");
    unwrap!(spawner.spawn(net_task(stack)));

    loop {
        //control.join_open(WIFI_NETWORK).await;
        match control.join_wpa2(WIFI_NETWORK, WIFI_PASSWORD).await {
            Ok(_) => break,
            Err(err) => {
                info!("join failed with status={}", err.status);
            }
        }
    }

    // Wait for DHCP, not necessary when using static IP
    info!("waiting for DHCP...");
    stack.wait_config_up().await;
    info!("DHCP is now up!");

    let command_sender = unsafe { command::COMMAND_CHANNEL.sender() };
    let mut command_receiver = unsafe { command::COMMAND_CHANNEL.receiver() };

    unwrap!(spawner.spawn(button_task(command_sender, led_sender.clone(), button_peripherals)));

    static RX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();
    let rx_buffer = RX_BUFFER.init([0; 4096]);
    static TX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();
    let tx_buffer = TX_BUFFER.init([0; 4096]);
    static PAYLOAD_BUFFER: StaticCell<heapless::Vec<u8, 4096>> = StaticCell::new();
    let payload_buffer = PAYLOAD_BUFFER.init(heapless::Vec::new());

    loop {
        if let Ok(dns_result) = stack.dns_query(HA_CONSTS.domain, DnsQueryType::A).await {
            if !dns_result.is_empty() {
                let socket = TcpSocket::new(stack, rx_buffer, tx_buffer);
                let mut websocket =
                    websocket::Websocket::new(socket, payload_buffer, &mut command_receiver, &mut led_sender);
                let endpoint = IpEndpoint::new(dns_result[0], HA_CONSTS.port);
                websocket.run(endpoint, HA_CONSTS.domain).await;
            }
        }

        const WAIT_SECS: u64 = 5;
        debug!("connection dropped, waiting {} seconds", WAIT_SECS);
        Timer::after_secs(WAIT_SECS).await;
    }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = embassy_rp::init(Default::default());

    let led_peripherals = led_peripherals!(take_peripheral_set, p);
    let button_peripherals = button_peripherals!(take_peripheral_set, p);
    let wifi_peripherals = wifi_peripherals!(take_peripheral_set, p);

    static mut CORE1_STACK: multicore::Stack<4096> = multicore::Stack::new();
    spawn_core1(p.CORE1, unsafe { &mut CORE1_STACK }, move || {
        static EXECUTOR1: StaticCell<Executor> = StaticCell::new();
        let executor1 = EXECUTOR1.init(Executor::new());
        let led_receiver = unsafe { leds::LED_CHANNEL.receiver() };
        executor1.run(|spawner| unwrap!(spawner.spawn(led_task(led_receiver, led_peripherals))));
    });

    static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
    let executor0 = EXECUTOR0.init(Executor::new());
    let led_sender = unsafe { leds::LED_CHANNEL.sender() };
    executor0
        .run(|spawner| unwrap!(spawner.spawn(core0_task(spawner, wifi_peripherals, button_peripherals, led_sender))));
}
