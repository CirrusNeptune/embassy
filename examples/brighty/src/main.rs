#![no_std]
#![no_main]

mod consts;
mod peripheral_macros;
mod sk6812;
mod udplisten;
mod color;
mod leds;

use cyw43_pio::PioSpi;
use defmt::{debug, info, unwrap};
use embassy_executor::{Executor, Spawner};
use embassy_net::{Config, DhcpConfig, IpAddress, IpEndpoint, Ipv4Address, Stack, StackResources};
use embassy_net::udp::{UdpSocket, PacketMetadata};
use embassy_rp::gpio::{Level, Output};
use embassy_rp::multicore;
use embassy_rp::multicore::spawn_core1;
use embassy_rp::peripherals::{DMA_CH0, I2C0, PIO0, PIO1};
use embassy_rp::{bind_interrupts, i2c, pio};
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};
use leds::{led_task, SK6812Peripherals};

const WIFI_SSID: &str = include_str!("../wifi_ssid.txt");
const WIFI_PSK: &[u8; 32] = include_bytes!("../wifi_psk.bin");

bind_interrupts!(struct Irqs {
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    PIO1_IRQ_0 => pio::InterruptHandler<PIO1>;
});

#[embassy_executor::task]
async fn wifi_task(runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>) -> ! {
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

    let mac = control.address().await;
    debug!("mac: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);

    loop {
        //control.join_open(WIFI_NETWORK).await;
        match control.join_wpa2_psk(WIFI_SSID, WIFI_PSK).await {
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

    let mut cmd_socket = {
        static RX_META: StaticCell<[PacketMetadata; 128]> = StaticCell::new();
        let rx_meta = RX_META.init([PacketMetadata::EMPTY; 128]);
        static RX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();
        let rx_buffer = RX_BUFFER.init([0; 4096]);
        static TX_META: StaticCell<[PacketMetadata; 0]> = StaticCell::new();
        let tx_meta = TX_META.init([PacketMetadata::EMPTY; 0]);
        static TX_BUFFER: StaticCell<[u8; 0]> = StaticCell::new();
        let tx_buffer = TX_BUFFER.init([0; 0]);

        UdpSocket::new(stack, rx_meta, rx_buffer, tx_meta, tx_buffer)
    };

    let mut discover_socket = {
        static RX_META: StaticCell<[PacketMetadata; 16]> = StaticCell::new();
        let rx_meta = RX_META.init([PacketMetadata::EMPTY; 16]);
        static RX_BUFFER: StaticCell<[u8; 512]> = StaticCell::new();
        let rx_buffer = RX_BUFFER.init([0; 512]);
        static TX_META: StaticCell<[PacketMetadata; 16]> = StaticCell::new();
        let tx_meta = TX_META.init([PacketMetadata::EMPTY; 16]);
        static TX_BUFFER: StaticCell<[u8; 512]> = StaticCell::new();
        let tx_buffer = TX_BUFFER.init([0; 512]);

        UdpSocket::new(stack, rx_meta, rx_buffer, tx_meta, tx_buffer)
    };

    unwrap!(cmd_socket.bind(consts::CMD_PORT));
    unwrap!(discover_socket.bind(consts::DISCOVER_PORT));

    udplisten::run(&mut cmd_socket, &mut discover_socket, &mac).await;
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = embassy_rp::init(Default::default());

    let wifi_peripherals = wifi_peripherals!(take_peripheral_set, p);
    let sk6812_peripherals = sk6812_peripherals!(take_peripheral_set, p);

    static mut CORE1_STACK: multicore::Stack<4096> = multicore::Stack::new();
    spawn_core1(p.CORE1, unsafe { &mut CORE1_STACK }, move || {
        static EXECUTOR1: StaticCell<Executor> = StaticCell::new();
        let executor1 = EXECUTOR1.init(Executor::new());
        let led_receiver = unsafe { leds::LED_CHANNEL.receiver() };
        executor1.run(|spawner| unwrap!(spawner.spawn(led_task(led_receiver, sk6812_peripherals))));
    });

    static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
    let executor0 = EXECUTOR0.init(Executor::new());
    executor0
        .run(|spawner| unwrap!(spawner.spawn(core0_task(spawner, wifi_peripherals))));
}
