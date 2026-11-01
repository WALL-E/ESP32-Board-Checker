#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod controller;
mod detector;
mod i18n;
mod view;

use controller::Controller;

fn main() -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;
    let controller = Controller::new(&window);
    controller.refresh_ports();

    let c = controller.clone();
    window.on_refresh_ports(move || c.refresh_ports());

    let c = controller.clone();
    window.on_select_port(move |index| c.select_port(index));

    let c = controller.clone();
    window.on_detect(move || c.detect());

    let c = controller.clone();
    window.on_apply_language(move || c.apply_language());

    window.run()
}
