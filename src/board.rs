use std::rc::Rc;

use anyhow::Result;
use slint::platform::software_renderer::{MinimalSoftwareWindow, PhysicalRegion, Rgb565Pixel};
use slint::{PhysicalSize, WindowSize};

use crate::app::App;
use crate::lcd::{Lcd, LCD_H_RES, LCD_V_RES};
use crate::touch::Touch;
use crate::xl9555::Xl9555;

pub struct Board {
    pub window: Rc<MinimalSoftwareWindow>,
    lcd: Lcd,
    touch: Touch,
    framebuffer: Vec<Rgb565Pixel>,
}

impl Board {
    pub fn new(window: Rc<MinimalSoftwareWindow>, mut xl9555: Xl9555) -> Result<Self> {
        window.set_size(WindowSize::Physical(PhysicalSize::new(
            LCD_H_RES.into(),
            LCD_V_RES.into(),
        )));

        let mut lcd = Lcd::new()?;

        lcd.set_direction_landscape()?;
        xl9555.set_lcd_backlight(true)?;
        let touch = Touch::new(xl9555)?;

        let framebuffer = vec![Rgb565Pixel(0); LCD_H_RES as usize * LCD_V_RES as usize];

        Ok(Self {
            window,
            lcd,
            touch,
            framebuffer,
        })
    }

    pub fn tick(&mut self, app: &App) -> Result<bool> {
        slint::platform::update_timers_and_animations();
        self.touch.poll(&self.window, app)?;

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
            return Ok(true);
        }

        Ok(false)
    }

    fn flush_dirty_region(
        &mut self,
        region: PhysicalRegion,
        framebuffer: &[u16],
    ) -> Result<()> {
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
