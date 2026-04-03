use anyhow::Result;
use std::rc::Rc;

use slint::platform::{PointerEventButton, WindowAdapter, WindowEvent};
use slint::platform::software_renderer::MinimalSoftwareWindow;
use slint::LogicalPosition;

use crate::lcd::{LCD_H_RES, LCD_V_RES};
use crate::xl9555::{Xl9555, CHSC5XXX_CTRL_REG};

pub struct Touch {
    pressed: bool,
    last_x: u16,
    last_y: u16,
    throttle: u8,
}

impl Touch {
    pub fn new() -> Self {
        Self {
            pressed: false,
            last_x: 0,
            last_y: 0,
            throttle: 0,
        }
    }

    pub fn init(&mut self, bus: &mut Xl9555) -> Result<()> {
        // 对应厂家 CT_RST 宏
        bus.set_touch_reset(false)?;
        std::thread::sleep(std::time::Duration::from_millis(20));
        bus.set_touch_reset(true)?;
        std::thread::sleep(std::time::Duration::from_millis(50));

        // 可选：读一下 PID，先不强依赖它
        let mut pid = [0u8; 4];
        let _ = bus.chsc5xxx_read_reg(crate::xl9555::CHSC5XXX_PID_REG, &mut pid);

        Ok(())
    }

    pub fn poll(
        &mut self,
        bus: &mut Xl9555,
        window: &Rc<MinimalSoftwareWindow>,
    ) -> Result<()> {
        // 对齐厂家代码：空闲时降低查询频率
        self.throttle = self.throttle.wrapping_add(1);
        if (self.throttle % 5) != 0 && self.throttle >= 5 {
            return Ok(());
        }

        let mut buf = [0u8; 28];
        bus.chsc5xxx_read_reg(CHSC5XXX_CTRL_REG, &mut buf)?;

        let count = buf[1] & 0x0F;

        if count > 0 {
            // 按厂家横屏公式
            let x = (((buf[5] >> 4) as u16) << 8) | buf[3] as u16;
            let y_raw = (((buf[5] & 0x0F) as u16) << 8) | buf[2] as u16;
            let y = LCD_V_RES.saturating_sub(y_raw);

            if x < LCD_H_RES && y < LCD_V_RES {
                let pos = LogicalPosition::new(x as f32, y as f32);

                if !self.pressed {
                    window.window().dispatch_event(WindowEvent::PointerPressed {
                        position: pos,
                        button: PointerEventButton::Left,
                    });
                    self.pressed = true;
                } else if x != self.last_x || y != self.last_y {
                    window
                        .window()
                        .dispatch_event(WindowEvent::PointerMoved { position: pos });
                }

                self.last_x = x;
                self.last_y = y;
                self.throttle = 0;
            }
        } else if self.pressed {
            let pos = LogicalPosition::new(self.last_x as f32, self.last_y as f32);

            window.window().dispatch_event(WindowEvent::PointerReleased {
                position: pos,
                button: PointerEventButton::Left,
            });

            self.pressed = false;
        }

        Ok(())
    }
}