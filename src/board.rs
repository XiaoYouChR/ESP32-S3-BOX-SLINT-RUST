use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Result;
use esp_idf_svc::hal::peripherals::Peripherals;
use log::info;
use slint::platform::software_renderer::{MinimalSoftwareWindow, PhysicalRegion, Rgb565Pixel};
use slint::{PhysicalSize, WindowSize};

use crate::app::App;
use crate::lcd::{Lcd, LCD_H_RES, LCD_V_RES};
use crate::touch::Touch;
use crate::xl9555::Xl9555;

const RENDER_STATS_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
struct FlushStats {
    rects: u32,
    pixels: u32,
}

struct RenderStats {
    window_start: Instant,
    frames: u32,
    total_render_time: Duration,
    max_render_time: Duration,
    total_flush_time: Duration,
    max_flush_time: Duration,
    total_flush_rects: u32,
    total_flush_pixels: u32,
}

impl RenderStats {
    fn new() -> Self {
        Self {
            window_start: Instant::now(),
            frames: 0,
            total_render_time: Duration::ZERO,
            max_render_time: Duration::ZERO,
            total_flush_time: Duration::ZERO,
            max_flush_time: Duration::ZERO,
            total_flush_rects: 0,
            total_flush_pixels: 0,
        }
    }

    fn record_frame(&mut self, render_time: Duration, flush_time: Duration, flush: FlushStats) {
        self.frames += 1;
        self.total_render_time += render_time;
        self.max_render_time = self.max_render_time.max(render_time);
        self.total_flush_time += flush_time;
        self.max_flush_time = self.max_flush_time.max(flush_time);
        self.total_flush_rects += flush.rects;
        self.total_flush_pixels += flush.pixels;
    }

    fn maybe_report(&mut self) {
        let elapsed = self.window_start.elapsed();
        if elapsed < RENDER_STATS_INTERVAL {
            return;
        }

        if self.frames > 0 {
            let fps = self.frames as f32 / elapsed.as_secs_f32();
            let avg_render_ms = duration_to_ms(self.total_render_time) / self.frames as f32;
            let avg_flush_ms = duration_to_ms(self.total_flush_time) / self.frames as f32;
            let max_render_ms = duration_to_ms(self.max_render_time);
            let max_flush_ms = duration_to_ms(self.max_flush_time);
            let avg_rects = self.total_flush_rects as f32 / self.frames as f32;
            let avg_pixels = self.total_flush_pixels as f32 / self.frames as f32;

            info!(
                "render stats: fps={:.1}, render avg/max={:.2}/{:.2} ms, flush avg/max={:.2}/{:.2} ms, rects/frame={:.1}, pixels/frame={:.0}",
                fps,
                avg_render_ms,
                max_render_ms,
                avg_flush_ms,
                max_flush_ms,
                avg_rects,
                avg_pixels,
            );
        }

        self.window_start = Instant::now();
        self.frames = 0;
        self.total_render_time = Duration::ZERO;
        self.max_render_time = Duration::ZERO;
        self.total_flush_time = Duration::ZERO;
        self.max_flush_time = Duration::ZERO;
        self.total_flush_rects = 0;
        self.total_flush_pixels = 0;
    }
}

fn duration_to_ms(duration: Duration) -> f32 {
    duration.as_secs_f32() * 1000.0
}

pub struct Board {
    pub window: Rc<MinimalSoftwareWindow>,
    lcd: Lcd,
    touch: Touch,
    framebuffer: Vec<Rgb565Pixel>,
    render_stats: RenderStats,
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

        lcd.set_direction_landscape()?;
        xl9555.set_lcd_backlight(true)?;
        let touch = Touch::new(xl9555)?;

        let framebuffer = vec![Rgb565Pixel(0); LCD_H_RES as usize * LCD_V_RES as usize];

        Ok(Self {
            window,
            lcd,
            touch,
            framebuffer,
            render_stats: RenderStats::new(),
        })
    }

    pub fn tick(&mut self, app: &App) -> Result<bool> {
        slint::platform::update_timers_and_animations();
        self.touch.poll(&self.window, app)?;

        let mut dirty_region = None;
        let mut render_time = Duration::ZERO;

        self.window.draw_if_needed(|renderer| {
            let render_start = Instant::now();
            let region = renderer.render(self.framebuffer.as_mut_slice(), LCD_H_RES as usize);
            render_time = render_start.elapsed();
            dirty_region = Some(region);
        });

        let rendered = dirty_region.is_some();

        if let Some(region) = dirty_region {
            let raw: &[u16] = unsafe {
                core::slice::from_raw_parts(
                    self.framebuffer.as_ptr() as *const u16,
                    self.framebuffer.len(),
                )
            };

            let flush_start = Instant::now();
            let flush_stats = self.flush_dirty_region(region, raw)?;
            let flush_time = flush_start.elapsed();

            self.render_stats
                .record_frame(render_time, flush_time, flush_stats);
        }

        self.render_stats.maybe_report();

        Ok(rendered)
    }

    fn flush_dirty_region(
        &mut self,
        region: PhysicalRegion,
        framebuffer: &[u16],
    ) -> Result<FlushStats> {
        let mut stats = FlushStats::default();

        for (origin, size) in region.iter() {
            if size.width == 0 || size.height == 0 {
                continue;
            }

            stats.rects += 1;
            stats.pixels += size.width * size.height;

            self.lcd.flush_rect_rgb565(
                origin.x as u16,
                origin.y as u16,
                size.width as u16,
                size.height as u16,
                LCD_H_RES as usize,
                framebuffer,
            )?;
        }

        Ok(stats)
    }
}
