use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Result;
use esp_idf_svc::hal::peripherals::Peripherals;
use slint::platform::software_renderer::{MinimalSoftwareWindow, PhysicalRegion, Rgb565Pixel};
use slint::{PhysicalSize, WindowSize};

use crate::app::App;
use crate::lcd::{Lcd, LCD_H_RES, LCD_V_RES};
use crate::touch::Touch;
use crate::xl9555::Xl9555;

const TOUCH_IDLE_FALLBACK_MS: u64 = 50;

pub struct Board {
    pub window: Rc<MinimalSoftwareWindow>,
    lcd: Lcd,
    xl9555: Xl9555,
    touch: Touch,
    framebuffer: Vec<Rgb565Pixel>,
    last_touch_idle_poll: Instant,
}

impl Board {
    pub fn new(window: Rc<MinimalSoftwareWindow>) -> Result<Self> {
        let peripherals = Peripherals::take().unwrap();

        window.set_size(WindowSize::Physical(PhysicalSize::new(
            LCD_H_RES.into(),
            LCD_V_RES.into(),
        )));

        let mut xl9555 = Xl9555::new(peripherals)?;
        let mut lcd = Lcd::new()?;
        let mut touch = Touch::new();

        lcd.set_direction_landscape()?;
        xl9555.set_lcd_backlight(true)?;
        touch.init(&mut xl9555)?;

        let framebuffer = vec![Rgb565Pixel(0); LCD_H_RES as usize * LCD_V_RES as usize];

        Ok(Self {
            window,
            lcd,
            xl9555,
            touch,
            framebuffer,
            last_touch_idle_poll: Instant::now(),
        })
    }

    pub fn tick(&mut self, app: &App) -> Result<()> {
        slint::platform::update_timers_and_animations();

        let touch_active = self.touch.is_active();
        let touch_irq = self.xl9555.take_touch_interrupt()?;
        let idle_fallback_due = !touch_active
            && self.last_touch_idle_poll.elapsed() >= Duration::from_millis(TOUCH_IDLE_FALLBACK_MS);

        if touch_active || touch_irq || idle_fallback_due {
            self.touch.poll(&mut self.xl9555, &self.window, app)?;

            if !self.touch.is_active() {
                self.last_touch_idle_poll = Instant::now();
            }
        }

        let mut dirty_region = None;

        self.window.draw_if_needed(|renderer| {
            let region = renderer.render(self.framebuffer.as_mut_slice(), LCD_H_RES as usize);
            dirty_region = Some(region);
        });

        if let Some(region) = dirty_region {
            let raw: &[u16] = unsafe {
                core::slice::from_raw_parts(
                    self.framebuffer.as_ptr() as *const u16,
                    self.framebuffer.len(),
                )
            };

            self.flush_dirty_region(region, raw)?;
        }

        Ok(())
    }

    fn flush_dirty_region(&mut self, region: PhysicalRegion, framebuffer: &[u16]) -> Result<()> {
        for (origin, size) in region.iter() {
            if size.width == 0 || size.height == 0 {
                continue;
            }

            self.lcd.flush_rect_rgb565(
                origin.x as u16,
                origin.y as u16,
                size.width as u16,
                size.height as u16,
                LCD_H_RES as usize,
                framebuffer,
            )?;
        }

        Ok(())
    }
}
