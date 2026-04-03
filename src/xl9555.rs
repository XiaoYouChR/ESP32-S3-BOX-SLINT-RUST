use anyhow::Result;
use esp_idf_svc::hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::units::Hertz;

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

pub struct Xl9555 {
    i2c: I2cDriver<'static>,
}

impl Xl9555 {
    pub fn new(peripherals: Peripherals) -> Result<Self> {
        let config = I2cConfig::new().baudrate(Hertz(400_000));

        // 按你板子的资源表：SDA=GPIO48, SCL=GPIO45
        let i2c = I2cDriver::new(
            peripherals.i2c0,
            peripherals.pins.gpio48,
            peripherals.pins.gpio45,
            &config,
        )?;

        let mut dev = Self { i2c };

        // 上电先读一次，清中断标志
        let mut r_data = [0u8; 2];
        dev.read_regs(XL9555_INPUT_PORT0_REG, &mut r_data)?;

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

    fn read_regs(&mut self, reg: u8, data: &mut [u8; 2]) -> Result<()> {
        self.i2c.write_read(XL9555_ADDR, &[reg], data, u32::MAX)?;
        Ok(())
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
}