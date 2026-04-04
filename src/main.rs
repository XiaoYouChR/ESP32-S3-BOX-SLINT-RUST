mod app;
mod board;
mod lcd;
mod touch;
mod xl9555;

use std::rc::Rc;
use std::time::{Duration, Instant};

use esp_idf_svc::log::EspLogger;
use log::info;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError};

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

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);

    slint::platform::set_platform(Box::new(EspPlatform {
        window: window.clone(),
        start: Instant::now(),
    }))
    .expect("failed to set Slint platform");

    let mut board = board::Board::new(window).expect("failed to init board");
    let app = app::create_ui(&board.window).expect("failed to create ui");

    info!("Entering main loop");

    loop {
        board.tick(&app).expect("board tick failed");
        std::thread::sleep(Duration::from_millis(10));
    }
}