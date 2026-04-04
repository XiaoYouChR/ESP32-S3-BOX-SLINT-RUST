use anyhow::{bail, Result};
use core::ffi::c_void;
use core::mem::zeroed;
use core::ptr;

use esp_idf_svc::sys::*;

pub const LCD_H_RES: u16 = 320;
pub const LCD_V_RES: u16 = 240;

const LCD_NUM_CS: i32 = 1;
const LCD_NUM_DC: i32 = 2;
const LCD_NUM_RD: i32 = 41;
const LCD_NUM_WR: i32 = 42;
const LCD_NUM_RST: i32 = -1;

const GPIO_LCD_D0: i32 = 40;
const GPIO_LCD_D1: i32 = 39;
const GPIO_LCD_D2: i32 = 38;
const GPIO_LCD_D3: i32 = 12;
const GPIO_LCD_D4: i32 = 11;
const GPIO_LCD_D5: i32 = 10;
const GPIO_LCD_D6: i32 = 9;
const GPIO_LCD_D7: i32 = 46;

pub struct Lcd {
    panel: esp_lcd_panel_handle_t,
    _io: esp_lcd_panel_io_handle_t,
    _bus: esp_lcd_i80_bus_handle_t,
}

impl Lcd {
    pub fn set_direction_landscape(&mut self) -> anyhow::Result<()> {
        // 0x60, 0xA0, 0x20, 0xE0, 0x68, 0xA8
        unsafe {
            let madctl = [0x60u8];
            esp!(esp_lcd_panel_io_tx_param(
                self._io,
                0x36,
                madctl.as_ptr() as *const c_void,
                1
            ))?;
        }
        Ok(())
    }

    pub fn new() -> Result<Self> {
        unsafe {
            let mut gpio_init: gpio_config_t = zeroed();
            gpio_init.intr_type = gpio_int_type_t_GPIO_INTR_DISABLE;
            gpio_init.mode = gpio_mode_t_GPIO_MODE_INPUT_OUTPUT;
            gpio_init.pin_bit_mask = 1u64 << LCD_NUM_RD;
            gpio_init.pull_down_en = gpio_pulldown_t_GPIO_PULLDOWN_DISABLE;
            gpio_init.pull_up_en = gpio_pullup_t_GPIO_PULLUP_ENABLE;
            esp!(gpio_config(&gpio_init))?;
            esp!(gpio_set_level(LCD_NUM_RD, 1))?;

            let mut bus_config: esp_lcd_i80_bus_config_t = zeroed();
            bus_config.clk_src = soc_module_clk_t_SOC_MOD_CLK_PLL_F160M;
            bus_config.dc_gpio_num = LCD_NUM_DC;
            bus_config.wr_gpio_num = LCD_NUM_WR;
            bus_config.data_gpio_nums = [
                GPIO_LCD_D0,
                GPIO_LCD_D1,
                GPIO_LCD_D2,
                GPIO_LCD_D3,
                GPIO_LCD_D4,
                GPIO_LCD_D5,
                GPIO_LCD_D6,
                GPIO_LCD_D7,
                -1,
                -1,
                -1,
                -1,
                -1,
                -1,
                -1,
                -1,
            ];
            bus_config.bus_width = 8;
            bus_config.max_transfer_bytes =
                LCD_H_RES as usize * LCD_V_RES as usize * core::mem::size_of::<u16>();
            bus_config.__bindgen_anon_1.psram_trans_align = 64;
            bus_config.sram_trans_align = 4;

            let mut bus: esp_lcd_i80_bus_handle_t = ptr::null_mut();
            esp!(esp_lcd_new_i80_bus(&bus_config, &mut bus))?;

            let mut io_config: esp_lcd_panel_io_i80_config_t = zeroed();
            io_config.cs_gpio_num = LCD_NUM_CS;
            io_config.pclk_hz = 10_000_000;
            io_config.trans_queue_depth = 10;
            io_config.on_color_trans_done = None;
            io_config.user_ctx = ptr::null_mut();
            io_config.lcd_cmd_bits = 8;
            io_config.lcd_param_bits = 8;

            io_config.dc_levels.set_dc_idle_level(0);
            io_config.dc_levels.set_dc_cmd_level(0);
            io_config.dc_levels.set_dc_dummy_level(0);
            io_config.dc_levels.set_dc_data_level(1);
            io_config.flags.set_swap_color_bytes(1);

            let mut io: esp_lcd_panel_io_handle_t = ptr::null_mut();
            esp!(esp_lcd_new_panel_io_i80(bus, &io_config, &mut io))?;

            let mut panel_config: esp_lcd_panel_dev_config_t = zeroed();
            panel_config.reset_gpio_num = LCD_NUM_RST;
            panel_config.bits_per_pixel = 16;
            panel_config.__bindgen_anon_1.rgb_ele_order =
                lcd_rgb_element_order_t_LCD_RGB_ELEMENT_ORDER_RGB;

            let mut panel: esp_lcd_panel_handle_t = ptr::null_mut();
            esp!(esp_lcd_new_panel_st7789(io, &panel_config, &mut panel))?;

            esp!(esp_lcd_panel_reset(panel))?;
            esp!(esp_lcd_panel_init(panel))?;
            esp!(esp_lcd_panel_invert_color(panel, true))?;
            esp!(esp_lcd_panel_set_gap(panel, 0, 0))?;

            let pixfmt = [0x65u8];
            esp!(esp_lcd_panel_io_tx_param(
                io,
                0x3A,
                pixfmt.as_ptr() as *const c_void,
                1
            ))?;

            let pixfmt = [0x65u8];
            esp!(esp_lcd_panel_io_tx_param(
                io,
                0x3A,
                pixfmt.as_ptr() as *const c_void,
                1
            ))?;

            esp!(esp_lcd_panel_disp_on_off(panel, true))?;

            Ok(Self {
                panel,
                _io: io,
                _bus: bus,
            })
        }
    }

    pub fn flush_rgb565(&mut self, width: u16, height: u16, pixels: &[u16]) -> Result<()> {
        let need = width as usize * height as usize;
        if pixels.len() < need {
            bail!("framebuffer too small: {} < {}", pixels.len(), need);
        }

        unsafe {
            esp!(esp_lcd_panel_draw_bitmap(
                self.panel,
                0,
                0,
                width as i32,
                height as i32,
                pixels.as_ptr() as *const c_void,
            ))?;
        }

        Ok(())
    }
}
