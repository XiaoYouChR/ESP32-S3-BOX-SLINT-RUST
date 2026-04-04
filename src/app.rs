slint::include_modules!();

pub fn create_ui(
    _window: &std::rc::Rc<slint::platform::software_renderer::MinimalSoftwareWindow>,
) -> Result<App, slint::PlatformError> {
    let app = App::new()?;
    app.set_counter(0);
    app.show()?;
    Ok(app)
}