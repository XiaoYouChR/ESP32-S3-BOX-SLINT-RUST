use anyhow::Result;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use esp_idf_svc::hal::cpu::Core;
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use log::{info, warn};
use slint::platform::software_renderer::MinimalSoftwareWindow;
use slint::platform::{PointerEventButton, WindowAdapter, WindowEvent};
use slint::{LogicalPosition, SharedString};

use crate::app::App;
use crate::lcd::{LCD_H_RES, LCD_V_RES};
use crate::xl9555::{Xl9555, CHSC5XXX_CTRL_REG, CHSC5XXX_PID_REG};

const TOUCH_READ_LEN: usize = 8;
// Keep touch sampling a bit above the UI frame budget without hammering the bus.
const TOUCH_WORKER_ACTIVE_POLL_MS: u64 = 6;
const TOUCH_WORKER_IDLE_CHECK_MS: u64 = 5;
const TOUCH_WORKER_IDLE_FALLBACK_MS: u64 = 50;
const MOVE_THRESHOLD: i16 = 2;
const TAP_SLOP: i16 = 10;
const LONG_PRESS_MS: u64 = 500;
const CHSC_EVENT_TYPE_TOUCH: u8 = 0xFF;
const CHSC_TOUCH_EVENT_RELEASE_BIT: u8 = 0x40;
const TOUCH_POINT_NONE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TouchState {
    Idle,
    Pressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
}

struct SharedTouchSample {
    sequence: AtomicU32,
    point_bits: AtomicU32,
}

impl SharedTouchSample {
    fn new() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            point_bits: AtomicU32::new(TOUCH_POINT_NONE),
        }
    }

    fn sequence(&self) -> u32 {
        self.sequence.load(Ordering::Acquire)
    }

    fn load_point(&self) -> Option<TouchPoint> {
        decode_touch_point(self.point_bits.load(Ordering::Relaxed))
    }

    fn publish(&self, point: Option<TouchPoint>) {
        self.point_bits
            .store(encode_touch_point(point), Ordering::Relaxed);
        self.sequence.fetch_add(1, Ordering::Release);
    }
}

pub struct Touch {
    shared_sample: Arc<SharedTouchSample>,
    stop_worker: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
    last_seen_sequence: u32,
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
    pub fn new(mut bus: Xl9555) -> Result<Self> {
        bus.set_touch_reset(false)?;
        std::thread::sleep(Duration::from_millis(20));
        bus.set_touch_reset(true)?;
        // The controller needs close to 100ms after reset before reads are reliable.
        std::thread::sleep(Duration::from_millis(100));

        let mut pid = [0u8; 4];
        let _ = bus.chsc5xxx_read_reg(CHSC5XXX_PID_REG, &mut pid);

        let shared_sample = Arc::new(SharedTouchSample::new());
        let stop_worker = Arc::new(AtomicBool::new(false));

        let restore_thread_conf = ThreadSpawnConfiguration::get().unwrap_or_default();
        let mut worker_thread_conf = ThreadSpawnConfiguration::default();
        worker_thread_conf.stack_size = restore_thread_conf.stack_size.max(6144);
        worker_thread_conf.priority = 4;
        worker_thread_conf.inherit = false;
        worker_thread_conf.pin_to_core = Some(Core::Core1);
        worker_thread_conf.set()?;

        let worker_handle = thread::Builder::new()
            .name("touch-worker".into())
            .spawn({
                let shared_sample = Arc::clone(&shared_sample);
                let stop_worker = Arc::clone(&stop_worker);

                move || touch_worker_loop(bus, shared_sample, stop_worker)
            })?;

        restore_thread_conf.set()?;

        Ok(Self {
            shared_sample,
            stop_worker,
            worker_handle: Some(worker_handle),
            last_seen_sequence: 0,
            state: TouchState::Idle,
            current_point: None,
            last_dispatch_point: None,
            press_origin: None,
            press_started_at: None,
            long_press_reported: false,
            tap_canceled: false,
            drag_active: false,
        })
    }

    pub fn poll(&mut self, window: &Rc<MinimalSoftwareWindow>, app: &App) -> Result<()> {
        let sequence = self.shared_sample.sequence();
        let has_new_sample = sequence != self.last_seen_sequence;

        if has_new_sample {
            self.last_seen_sequence = sequence;
        }

        if has_new_sample {
            match (self.state, self.shared_sample.load_point()) {
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
        } else if let Some(point) = self.current_point {
            if self.state == TouchState::Pressed && self.should_report_long_press(point) {
                self.long_press_reported = true;
                self.tap_canceled = true;
                self.report_long_press(app, point);
            }
        }

        Ok(())
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

impl Drop for Touch {
    fn drop(&mut self) {
        self.stop_worker.store(true, Ordering::Release);

        if let Some(worker_handle) = self.worker_handle.take() {
            let _ = worker_handle.join();
        }
    }
}

fn touch_worker_loop(
    mut bus: Xl9555,
    shared_sample: Arc<SharedTouchSample>,
    stop_worker: Arc<AtomicBool>,
) {
    let mut touch_active = false;
    let mut last_active_poll = Instant::now();
    let mut last_idle_fallback = Instant::now();
    let mut last_point = None;

    while !stop_worker.load(Ordering::Acquire) {
        let touch_irq = match bus.take_touch_interrupt() {
            Ok(pending) => pending,
            Err(err) => {
                warn!("touch IRQ check failed: {err:#}");
                false
            }
        };

        let active_poll_due = touch_active
            && last_active_poll.elapsed() >= Duration::from_millis(TOUCH_WORKER_ACTIVE_POLL_MS);
        let idle_fallback_due = !touch_active
            && last_idle_fallback.elapsed() >= Duration::from_millis(TOUCH_WORKER_IDLE_FALLBACK_MS);
        let should_read = if touch_active {
            active_poll_due
        } else {
            touch_irq || idle_fallback_due
        };

        if should_read {
            match read_sample_from_bus(&mut bus) {
                Ok(point) => {
                    touch_active = point.is_some();
                    last_active_poll = Instant::now();
                    if !touch_active {
                        last_idle_fallback = Instant::now();
                    }

                    let changed = point != last_point;

                    if changed {
                        last_point = point;
                        shared_sample.publish(point);
                    }
                }
                Err(err) => warn!("touch sample read failed: {err:#}"),
            }
        }

        thread::sleep(Duration::from_millis(if touch_active {
            TOUCH_WORKER_ACTIVE_POLL_MS
        } else {
            TOUCH_WORKER_IDLE_CHECK_MS
        }));
    }
}

fn read_sample_from_bus(bus: &mut Xl9555) -> Result<Option<TouchPoint>> {
    let mut buf = [0u8; TOUCH_READ_LEN];
    bus.chsc5xxx_read_reg(CHSC5XXX_CTRL_REG, &mut buf)?;

    if buf[0] != CHSC_EVENT_TYPE_TOUCH {
        return Ok(None);
    }

    let count = buf[1] & 0x0F;
    let released = (buf[6] & CHSC_TOUCH_EVENT_RELEASE_BIT) != 0;
    if count == 0 || released {
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

fn encode_touch_point(point: Option<TouchPoint>) -> u32 {
    match point {
        Some(point) => ((point.x as u32) << 16) | point.y as u32,
        None => TOUCH_POINT_NONE,
    }
}

fn decode_touch_point(bits: u32) -> Option<TouchPoint> {
    if bits == TOUCH_POINT_NONE {
        None
    } else {
        Some(TouchPoint {
            x: (bits >> 16) as u16,
            y: bits as u16,
        })
    }
}
