use std::rc::Rc;
use std::time::{Duration, Instant};

use esp_idf_svc::log::EspLogger;
use log::info;

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError};

slint::include_modules!();

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

    info!("Initializing Slint platform");

    let platform = EspPlatform {
        window: MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer),
        start: Instant::now(),
    };

    slint::platform::set_platform(Box::new(platform)).expect("failed to set Slint platform");

    info!("Starting Slint integration test");

    let app = App::new().expect("failed to create Slint app");
    app.set_counter(1);

    info!("Slint UI object created successfully");
}