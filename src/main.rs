mod app;
mod board;
mod lcd;
mod touch;
mod wifi;
mod xl9555;

use std::rc::Rc;
use std::time::{Duration, Instant};

use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::info;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError};

// const MAIN_LOOP_BUSY_SLEEP_MS: u64 = 1;
const MAIN_LOOP_IDLE_SLEEP_MS: u64 = 5;

struct EspPlatform {
    window: Rc<MinimalSoftwareWindow>,
    start: Instant,
}

impl Platform for EspPlatform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("Booting...");

    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);

    slint::platform::set_platform(Box::new(EspPlatform {
        window: window.clone(),
        start: Instant::now(),
    }))
    .expect("failed to set Slint platform");

    let peripherals = Peripherals::take().expect("failed to take peripherals");
    let xl9555 = xl9555::Xl9555::new(
        peripherals.i2c0,
        peripherals.pins.gpio48,
        peripherals.pins.gpio45,
        peripherals.pins.gpio3,
    )
    .expect("failed to init xl9555");

    let mut board = board::Board::new(window, xl9555).expect("failed to init board");
    let app = app::create_ui(&board.window).expect("failed to create ui");

    info!("Entering main loop");

    loop {
        let rendered = board.tick(&app).expect("board tick failed");
        if rendered {
            // std::thread::sleep(Duration::from_millis(MAIN_LOOP_BUSY_SLEEP_MS));
            std::thread::yield_now();
        } else {
            std::thread::sleep(Duration::from_millis(MAIN_LOOP_IDLE_SLEEP_MS));
        }
    }
}
