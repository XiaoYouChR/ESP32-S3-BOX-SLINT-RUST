use esp_idf_svc::log::EspLogger;
use log::info;

slint::include_modules!();

fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    EspLogger::initialize_default();
    
    let app = App::new().unwrap();
    app.set_counter(1);

    info!("Slint UI object created successfully");
}
