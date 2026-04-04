use anyhow::Result;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::platform::software_renderer::MinimalSoftwareWindow;
use slint::platform::{PointerEventButton, WindowEvent, WindowAdapter};
use slint::LogicalPosition;

use crate::lcd::{LCD_H_RES, LCD_V_RES};
use crate::xl9555::{Xl9555, CHSC5XXX_CTRL_REG, CHSC5XXX_PID_REG};

const TOUCH_READ_LEN: usize = 28;
const MOVE_THRESHOLD: i16 = 2;
const RELEASE_DEBOUNCE_MS: u64 = 20;

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
    last_point: Option<TouchPoint>,
    throttle: u8,
    last_release_check: Instant,
}

impl Touch {
    pub fn new() -> Self {
        Self {
            state: TouchState::Idle,
            last_point: None,
            throttle: 0,
            last_release_check: Instant::now(),
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
    ) -> Result<()> {
        self.throttle = self.throttle.wrapping_add(1);
        if (self.throttle % 5) != 0 && self.throttle >= 5 {
            return Ok(());
        }

        let sample = self.read_sample(bus)?;

        match (self.state, sample) {
            (TouchState::Idle, Some(point)) => {
                self.dispatch_pressed(window, point);
                self.state = TouchState::Pressed;
                self.last_point = Some(point);
                self.throttle = 0;
            }

            (TouchState::Pressed, Some(point)) => {
                if self.should_send_move(point) {
                    self.dispatch_moved(window, point);
                    self.last_point = Some(point);
                }
                self.throttle = 0;
            }

            (TouchState::Pressed, None) => {
                if self.last_release_check.elapsed() >= Duration::from_millis(RELEASE_DEBOUNCE_MS) {
                    if let Some(point) = self.last_point {
                        self.dispatch_released(window, point);
                    }
                    self.state = TouchState::Idle;
                    self.last_point = None;
                    self.last_release_check = Instant::now();
                }
            }

            (TouchState::Idle, None) => {}
        }

        Ok(())
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
        match self.last_point {
            None => true,
            Some(last) => {
                let dx = point.x as i16 - last.x as i16;
                let dy = point.y as i16 - last.y as i16;
                dx.abs() >= MOVE_THRESHOLD || dy.abs() >= MOVE_THRESHOLD
            }
        }
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
        window.window().dispatch_event(WindowEvent::PointerReleased {
            position: pos,
            button: PointerEventButton::Left,
        });
    }
}