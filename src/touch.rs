use anyhow::Result;
use std::rc::Rc;
use std::time::{Duration, Instant};

use log::info;
use slint::platform::software_renderer::MinimalSoftwareWindow;
use slint::platform::{PointerEventButton, WindowAdapter, WindowEvent};
use slint::{LogicalPosition, SharedString};

use crate::app::App;
use crate::lcd::{LCD_H_RES, LCD_V_RES};
use crate::xl9555::{Xl9555, CHSC5XXX_CTRL_REG, CHSC5XXX_PID_REG};

const TOUCH_READ_LEN: usize = 28;
const MOVE_THRESHOLD: i16 = 2;
const TAP_SLOP: i16 = 10;
const LONG_PRESS_MS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TouchState {
    Idle,
    Pressed,
}

#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
}

pub struct Touch {
    state: TouchState,
    current_point: Option<TouchPoint>,
    last_dispatch_point: Option<TouchPoint>,
    press_origin: Option<TouchPoint>,
    press_started_at: Option<Instant>,
    long_press_reported: bool,
    tap_canceled: bool,
    drag_active: bool,
}

impl Touch {
    pub fn new() -> Self {
        Self {
            state: TouchState::Idle,
            current_point: None,
            last_dispatch_point: None,
            press_origin: None,
            press_started_at: None,
            long_press_reported: false,
            tap_canceled: false,
            drag_active: false,
        }
    }

    pub fn init(&mut self, bus: &mut Xl9555) -> Result<()> {
        bus.set_touch_reset(false)?;
        std::thread::sleep(Duration::from_millis(20));
        bus.set_touch_reset(true)?;
        std::thread::sleep(Duration::from_millis(50));

        let mut pid = [0u8; 4];
        let _ = bus.chsc5xxx_read_reg(CHSC5XXX_PID_REG, &mut pid);

        Ok(())
    }

    pub fn poll(
        &mut self,
        bus: &mut Xl9555,
        window: &Rc<MinimalSoftwareWindow>,
        app: &App,
    ) -> Result<()> {
        let sample = self.read_sample(bus)?;

        match (self.state, sample) {
            (TouchState::Idle, Some(point)) => {
                self.dispatch_pressed(window, point);
                self.state = TouchState::Pressed;
                self.current_point = Some(point);
                self.last_dispatch_point = Some(point);
                self.press_origin = Some(point);
                self.press_started_at = Some(Instant::now());
                self.long_press_reported = false;
                self.tap_canceled = false;
                self.drag_active = false;
            }

            (TouchState::Pressed, Some(point)) => {
                self.current_point = Some(point);

                if self.moved_past_tap_slop(point) {
                    self.tap_canceled = true;
                    self.drag_active = true;
                }

                if self.should_report_long_press(point) {
                    self.long_press_reported = true;
                    self.tap_canceled = true;
                    self.report_long_press(app, point);
                }

                if self.drag_active && self.should_send_move(point) {
                    self.dispatch_moved(window, point);
                    self.last_dispatch_point = Some(point);
                }
            }

            (TouchState::Pressed, None) => {
                if let Some(point) = self.current_point {
                    if self.tap_canceled {
                        self.dispatch_canceled_release(window, point);
                    } else {
                        self.dispatch_released(window, point);
                    }
                }
                self.reset_tracking();
            }

            (TouchState::Idle, None) => {}
        }

        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.state == TouchState::Pressed
    }

    fn read_sample(&self, bus: &mut Xl9555) -> Result<Option<TouchPoint>> {
        let mut buf = [0u8; TOUCH_READ_LEN];
        bus.chsc5xxx_read_reg(CHSC5XXX_CTRL_REG, &mut buf)?;

        let count = buf[1] & 0x0F;
        if count == 0 {
            return Ok(None);
        }

        // 按厂家横屏公式
        let x = (((buf[5] >> 4) as u16) << 8) | buf[3] as u16;
        let y_raw = (((buf[5] & 0x0F) as u16) << 8) | buf[2] as u16;
        let y = LCD_V_RES.saturating_sub(y_raw);

        if x >= LCD_H_RES || y >= LCD_V_RES {
            return Ok(None);
        }

        Ok(Some(TouchPoint { x, y }))
    }

    fn should_send_move(&self, point: TouchPoint) -> bool {
        match self.last_dispatch_point {
            None => true,
            Some(last) => {
                let dx = point.x as i16 - last.x as i16;
                let dy = point.y as i16 - last.y as i16;
                dx.abs() >= MOVE_THRESHOLD || dy.abs() >= MOVE_THRESHOLD
            }
        }
    }

    fn moved_past_tap_slop(&self, point: TouchPoint) -> bool {
        match self.press_origin {
            None => false,
            Some(origin) => {
                let dx = point.x as i16 - origin.x as i16;
                let dy = point.y as i16 - origin.y as i16;
                dx.abs() >= TAP_SLOP || dy.abs() >= TAP_SLOP
            }
        }
    }

    fn should_report_long_press(&self, point: TouchPoint) -> bool {
        !self.long_press_reported
            && !self.moved_past_tap_slop(point)
            && self
                .press_started_at
                .is_some_and(|started| started.elapsed() >= Duration::from_millis(LONG_PRESS_MS))
    }

    fn report_long_press(&self, app: &App, point: TouchPoint) {
        let count = app.get_long_press_count().saturating_add(1);
        app.set_long_press_count(count);
        app.set_last_gesture(SharedString::from(format!(
            "Long press @ ({}, {})",
            point.x, point.y
        )));
        info!("touch long press at ({}, {})", point.x, point.y);
    }

    fn reset_tracking(&mut self) {
        self.state = TouchState::Idle;
        self.current_point = None;
        self.last_dispatch_point = None;
        self.press_origin = None;
        self.press_started_at = None;
        self.long_press_reported = false;
        self.tap_canceled = false;
        self.drag_active = false;
    }

    fn dispatch_pressed(&self, window: &Rc<MinimalSoftwareWindow>, point: TouchPoint) {
        let pos = LogicalPosition::new(point.x as f32, point.y as f32);
        window.window().dispatch_event(WindowEvent::PointerPressed {
            position: pos,
            button: PointerEventButton::Left,
        });
    }

    fn dispatch_moved(&self, window: &Rc<MinimalSoftwareWindow>, point: TouchPoint) {
        let pos = LogicalPosition::new(point.x as f32, point.y as f32);
        window
            .window()
            .dispatch_event(WindowEvent::PointerMoved { position: pos });
    }

    fn dispatch_released(&self, window: &Rc<MinimalSoftwareWindow>, point: TouchPoint) {
        let pos = LogicalPosition::new(point.x as f32, point.y as f32);
        window
            .window()
            .dispatch_event(WindowEvent::PointerReleased {
                position: pos,
                button: PointerEventButton::Left,
            });
    }

    fn dispatch_canceled_release(&self, window: &Rc<MinimalSoftwareWindow>, point: TouchPoint) {
        window.window().dispatch_event(WindowEvent::PointerExited);
        self.dispatch_released(window, point);
    }
}
