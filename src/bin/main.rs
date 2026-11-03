#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use core::sync::atomic::{AtomicU8, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::Async;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use flight_computer_2026_27::flight::{FlightComputer, FlightState, SensorSample};
use flight_computer_2026_27::lora::Radio;
use flight_computer_2026_27::oled::Oled;
use flight_computer_2026_27::power::Power;
use flight_computer_2026_27::pins;
use log::{info, warn};

extern crate alloc;

static FLIGHT_STATE: AtomicU8 = AtomicU8::new(0);

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 1.4.0
    // generator parameters: -o esp32s3 -o unstable-hal -o alloc -o embassy -o stack-smashing-protection -o log -o esp-backtrace -o helix -o vscode -o esp

    esp_println::logger::init_logger(log::LevelFilter::Info);

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    info!("Embassy initialized!");
    info!("Main Loop Starting...");

    // pin numbers below must match `pins::PMU_SDA`/`PMU_SCL`.
    let pmu_i2c = I2c::new(peripherals.I2C1, I2cConfig::default())
        .expect("failed to configure PMU I2C bus")
        .with_sda(peripherals.GPIO42)
        .with_scl(peripherals.GPIO41)
        .into_async();

    let mut power = Power::new(pmu_i2c);
    power
        .power_on_all()
        .await
        .expect("failed to power on LoRa/GPS/OLED rails");

    if let Err(_e) = power.enable_telemetry().await {
        warn!("failed to enable PMU telemetry ADCs; power stats will be stale");
    }

    // pin numbers must match `pins::OLED_SDA`/`OLED_SCL`.
    let oled_i2c = I2c::new(peripherals.I2C0, I2cConfig::default())
        .expect("failed to configure OLED I2C bus")
        .with_sda(peripherals.GPIO17)
        .with_scl(peripherals.GPIO18)
        .into_async();

    // pin numbers must match `pins::LORA_SCK`/`LORA_MISO`/`LORA_MOSI`.
    let lora_spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(4))
            .with_mode(SpiMode::_0),
    )
    .expect("failed to configure LoRa SPI bus")
    .with_sck(peripherals.GPIO12)
    .with_mosi(peripherals.GPIO11)
    .with_miso(peripherals.GPIO13)
    .into_async();

    let lora_nss = Output::new(peripherals.GPIO10, Level::High, OutputConfig::default());
    let lora_reset = Output::new(peripherals.GPIO5, Level::High, OutputConfig::default());
    let lora_dio1 = Input::new(peripherals.GPIO1, InputConfig::default());
    let lora_busy = Input::new(peripherals.GPIO4, InputConfig::default());

    let radio = match Radio::new(lora_spi, lora_nss, lora_reset, lora_dio1, lora_busy).await {
        Ok(radio) => Some(radio),
        Err(e) => {
            warn!("LoRa radio init failed: {:?}; continuing without broadcast", e);
            None
        }
    };

    match Oled::new(oled_i2c).await {
        Ok(oled) => {
            spawner.spawn(
                telemetry_display_task(power, oled, radio)
                    .expect("failed to spawn OLED display task"),
            );
        }
        Err(_e) => {
            warn!("OLED init failed (checked {:#x}/{:#x}); continuing without display",
                pins::OLED_I2C_ADDRESS_PRIMARY, pins::OLED_I2C_ADDRESS_FALLBACK);
        }
    }
    spawner.spawn(
        flight_loop_task().expect("failed to spawn flight loop task"),
    );

    let loop_rate_seconds = 1;

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.2.2/examples
    info!("Main Loop Started!");
    info!("Loop Rate: {loop_rate_seconds} seconds");
    loop {
        Timer::after(Duration::from_secs(loop_rate_seconds)).await;
    }
}

// TODO: add ground comms task

/// currently fed a zeroed `SensorSample` need to replace with real imu/
/// barometer readings once a driver for that exists.
#[embassy_executor::task]
async fn flight_loop_task() -> ! {
    let mut flight = FlightComputer::default();
    let mut last_state = flight.state();

    loop {
        let sample = SensorSample::default();
        let state = flight.update(sample);
        FLIGHT_STATE.store(state.to_u8(), Ordering::Relaxed);

        if state != last_state {
            info!("flight state: {:?} -> {:?}", last_state, state);
            last_state = state;
        }

        Timer::after(Duration::from_millis(20)).await;
    }
}

#[allow(
    clippy::large_stack_frames,
    reason = "the OLED framebuffer (128x8 bytes), PMU driver, and LoRa radio/SPI state live \
    in this task's state"
)]
#[embassy_executor::task]
async fn telemetry_display_task(
    mut power: Power<I2c<'static, Async>>,
    mut oled: Oled<I2c<'static, Async>>,
    mut radio: Option<Radio>,
) -> ! {
    loop {
        let telemetry = power.read_telemetry().await.unwrap_or_else(|_| {
            warn!("failed to read PMU telemetry");
            Default::default()
        });
        let flight_state = FlightState::from_u8(FLIGHT_STATE.load(Ordering::Relaxed));

        if let Some(radio) = radio.as_mut() {
            radio.broadcast(flight_state, &telemetry).await;
        }

        let radio_status = radio.as_ref().map(|r| r.status());
        if let Err(_e) = oled
            .render(flight_state, &telemetry, radio_status)
            .await
        {
            warn!("failed to update OLED");
        }

        Timer::after(Duration::from_millis(500)).await;
    }
}
