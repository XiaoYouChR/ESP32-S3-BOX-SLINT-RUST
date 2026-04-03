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

    window.set_size(WindowSize::Physical(PhysicalSize::new(320, 240)));

    let app = App::new().expect("failed to create Slint app");
    app.set_counter(1);
    app.show().expect("failed to show app");

    info!("Rendering first frame");

    let mut framebuffer = vec![Rgb565Pixel(0); 320 * 240];

    window.draw_if_needed(|renderer| {
        renderer.render(framebuffer.as_mut_slice(), 320);
    });

    info!("Rendered first frame OK");

    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}