use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use esp_idf_svc::log::EspLogger;
use log::info;

use slint::platform::software_renderer::{
    MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel,
};
use slint::platform::{Platform, PlatformError};
use slint::{PhysicalSize, WindowSize};

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

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);

    let platform = EspPlatform {
        window: window.clone(),
        start: Instant::now(),
    };

    slint::platform::set_platform(Box::new(platform)).expect("failed to set Slint platform");

    info!("Starting Slint integration test");

    window.set_size(WindowSize::Physical(PhysicalSize::new(320, 240)));

    let app = App::new().expect("failed to create Slint app");
    app.set_counter(1);
    app.show().expect("failed to show app");

    info!("Slint UI object created successfully");

    let framebuffer = RefCell::new(vec![Rgb565Pixel(0); 320 * 240]);

    window.draw_if_needed(|renderer| {
        renderer.render(framebuffer.borrow_mut().as_mut_slice(), 320);
    });

    let fb = framebuffer.borrow();

    info!("Rendered first frame");
    info!("Framebuffer size: {} pixels", fb.len());
    info!(
        "First pixels: {:04x} {:04x} {:04x} {:04x}",
        fb[0].0, fb[1].0, fb[2].0, fb[3].0
    );
}