use anyhow::Result;
use core::sync::atomic::{AtomicBool, Ordering};
use esp_idf_svc::hal::gpio::{Input, InputPin, InterruptType, OutputPin, PinDriver, Pull};
use esp_idf_svc::hal::i2c::{I2cConfig, I2cDriver, I2c};
use esp_idf_svc::hal::units::Hertz;

const TOUCH_INT_ACTIVE_LEVEL: bool = false;
static XL9555_IRQ_PENDING: AtomicBool = AtomicBool::new(false);

pub const XL9555_ADDR: u8 = 0x20;

pub const XL9555_INPUT_PORT0_REG: u8 = 0;
pub const XL9555_INPUT_PORT1_REG: u8 = 1;
pub const XL9555_OUTPUT_PORT0_REG: u8 = 2;
pub const XL9555_OUTPUT_PORT1_REG: u8 = 3;
pub const XL9555_INVERSION_PORT0_REG: u8 = 4;
pub const XL9555_INVERSION_PORT1_REG: u8 = 5;
pub const XL9555_CONFIG_PORT0_REG: u8 = 6;
pub const XL9555_CONFIG_PORT1_REG: u8 = 7;

pub const AP_INT_IO: u16 = 0x0001;
pub const QMA_INT_IO: u16 = 0x0002;
pub const BEEP_IO: u16 = 0x0004;
pub const KEY1_IO: u16 = 0x0008;
pub const KEY0_IO: u16 = 0x0010;
pub const SPK_CTRL_IO: u16 = 0x0020;
pub const CTP_RST_IO: u16 = 0x0040;
pub const LCD_BL_IO: u16 = 0x0080;
pub const LEDR_IO: u16 = 0x0100;
pub const CTP_INT_IO: u16 = 0x0200;
pub const CHSC5432_ADDR: u8 = 0x2E;
pub const CHSC5XXX_CTRL_REG: u32 = 0x2000_002C;
pub const CHSC5XXX_PID_REG: u32 = 0x2000_0080;

pub struct Xl9555 {
    i2c: I2cDriver<'static>,
    irq: PinDriver<'static, Input>,
}

impl Xl9555 {
    pub fn new<I2C: I2c + 'static>(
        i2c0: I2C,
        sda: impl InputPin + OutputPin + 'static,
        scl: impl InputPin + OutputPin + 'static,
        irq_pin: impl InputPin + 'static,
    ) -> Result<Self> {
        let config = I2cConfig::new().baudrate(Hertz(400_000));

        let i2c = I2cDriver::new(i2c0, sda, scl, &config)?;

        let mut irq = PinDriver::input(irq_pin, Pull::Up)?;
        irq.set_interrupt_type(InterruptType::NegEdge)?;
        unsafe {
            irq.subscribe(|| {
                XL9555_IRQ_PENDING.store(true, Ordering::Release);
            })?;
        }
        irq.enable_interrupt()?;

        let mut dev = Self { i2c, irq };

        // 上电先读一次，清中断标志
        let _ = dev.read_input_state()?;

        // 厂家初始化：配置 IO 方向
        // 0xFE1B => P0=0x1B, P1=0xFE
        dev.io_config(0xFE1B)?;

        // 芯片初始化时先关闭蜂鸣器
        dev.pin_write(BEEP_IO, true)?;

        // 关闭喇叭
        dev.pin_write(SPK_CTRL_IO, false)?;

        Ok(dev)
    }

    fn write_reg(&mut self, reg: u8, data: &[u8]) -> Result<()> {
        let mut buf = Vec::with_capacity(1 + data.len());
        buf.push(reg);
        buf.extend_from_slice(data);
        self.i2c.write(XL9555_ADDR, &buf, u32::MAX)?;
        Ok(())
    }

    fn read_regs(&mut self, reg: u8, data: &mut [u8]) -> Result<()> {
        self.i2c.write_read(XL9555_ADDR, &[reg], data, u32::MAX)?;
        Ok(())
    }

    pub fn read_input_state(&mut self) -> Result<u16> {
        let mut data = [0u8; 2];
        self.read_regs(XL9555_INPUT_PORT0_REG, &mut data)?;
        Ok(((data[1] as u16) << 8) | data[0] as u16)
    }

    pub fn io_config(&mut self, value: u16) -> Result<()> {
        let data = [(value & 0x00FF) as u8, ((value >> 8) & 0x00FF) as u8];
        self.write_reg(XL9555_CONFIG_PORT0_REG, &data)
    }

    pub fn pin_write(&mut self, pin: u16, val: bool) -> Result<u16> {
        let mut w_data = [0u8; 2];

        // 厂家代码这里是从输入寄存器 0 开始读两个字节
        // 但用于改输出位时，读 OUTPUT_PORT 更稳妥
        self.read_regs(XL9555_OUTPUT_PORT0_REG, &mut w_data)?;

        if pin <= 0x00FF {
            if val {
                w_data[0] |= (pin & 0x00ff) as u8;
            } else {
                w_data[0] &= !((pin & 0x00ff) as u8);
            }
        } else if val {
            w_data[1] |= ((pin >> 8) & 0x00ff) as u8;
        } else {
            w_data[1] &= !(((pin >> 8) & 0x00ff) as u8);
        }

        self.write_reg(XL9555_OUTPUT_PORT0_REG, &w_data)?;
        Ok(((w_data[1] as u16) << 8) | w_data[0] as u16)
    }

    pub fn set_lcd_backlight(&mut self, on: bool) -> Result<()> {
        self.pin_write(LCD_BL_IO, on)?;
        Ok(())
    }

    pub fn set_touch_reset(&mut self, on: bool) -> Result<()> {
        self.pin_write(CTP_RST_IO, on)?;
        Ok(())
    }

    pub fn take_touch_interrupt(&mut self) -> Result<bool> {
        let pending = XL9555_IRQ_PENDING.swap(false, Ordering::AcqRel) || self.irq.is_low();
        if !pending {
            return Ok(false);
        }

        let inputs = self.read_input_state()?;
        let touch_line_active = ((inputs & CTP_INT_IO) != 0) == TOUCH_INT_ACTIVE_LEVEL;

        // PinDriver 会在 ISR 触发后自动关中断，这里在非 ISR 上下文重新打开。
        self.irq.enable_interrupt()?;

        // 只要 XL9555 确实给过一次 IRQ，我们就至少安排一次触摸扫描；
        // CTP_INT_IO 低电平则说明触摸中断仍在当前时刻保持有效。
        Ok(pending || touch_line_active)
    }

    pub fn chsc5xxx_read_reg(&mut self, reg: u32, buf: &mut [u8]) -> Result<()> {
        // 这里先按大端寄存器地址发送：
        // 0x2000002C -> [0x20, 0x00, 0x00, 0x2C]
        // 如果后面读不到有效数据，再试 to_le_bytes()
        let addr = reg.to_be_bytes();
        self.i2c.write_read(CHSC5432_ADDR, &addr, buf, u32::MAX)?;
        Ok(())
    }
}
