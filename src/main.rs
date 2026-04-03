use std::rc::Rc;
use std::time::{Duration, Instant};

use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::info;

use slint::platform::software_renderer::{
    MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel,
};
use slint::platform::{Platform, PlatformError};
use slint::{PhysicalSize, WindowSize};

mod lcd;
mod xl9555;

use lcd::{Lcd, LCD_H_RES, LCD_V_RES};
use xl9555::Xl9555;

mod touch;
use touch::Touch;

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

    let peripherals = Peripherals::take().unwrap();

    info!("Initializing Slint platform");

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);

    let platform = EspPlatform {
        window: window.clone(),
        start: Instant::now(),
    };

    slint::platform::set_platform(Box::new(platform)).expect("failed to set Slint platform");

    window.set_size(WindowSize::Physical(PhysicalSize::new(
        LCD_H_RES.into(),
        LCD_V_RES.into(),
    )));

    let app = App::new().expect("failed to create Slint app");
    app.set_counter(1);
    app.show().expect("failed to show app");

    let mut framebuffer = vec![Rgb565Pixel(0); LCD_H_RES as usize * LCD_V_RES as usize];

    let mut xl9555 = Xl9555::new(peripherals).expect("failed to init xl9555");
    let mut lcd = Lcd::new().expect("failed to init lcd");
    let mut touch = Touch::new();

    xl9555
        .set_lcd_backlight(true)
        .expect("failed to enable backlight");

    lcd.set_direction_landscape()
        .expect("failed to set lcd direction");

    touch.init(&mut xl9555).expect("failed to init touch");

    info!("Entering UI loop");

    loop {
        slint::platform::update_timers_and_animations();

        if let Err(e) = touch.poll(&mut xl9555, &window) {
            log::warn!("touch poll failed: {:?}", e);
        }

        let mut rendered = false;

        window.draw_if_needed(|renderer| {
            renderer.render(framebuffer.as_mut_slice(), LCD_H_RES as usize);
            rendered = true;
        });

        if rendered {
            let raw: &[u16] = unsafe {
                core::slice::from_raw_parts(framebuffer.as_ptr() as *const u16, framebuffer.len())
            };

            lcd.flush_rgb565(LCD_H_RES, LCD_V_RES, raw)
                .expect("failed to flush framebuffer");
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}