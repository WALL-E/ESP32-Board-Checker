#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod detector;

use std::thread;
use std::time::{Duration, Instant};

use slint::{Model, ModelRc, VecModel};

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    refresh_ports(&ui);

    let weak = ui.as_weak();
    ui.on_refresh_ports(move || {
        if let Some(ui) = weak.upgrade() {
            refresh_ports(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_select_port(move |index| {
        if let Some(ui) = weak.upgrade() {
            let ports = ui.get_ports();
            let Some(entry) = ports.row_data(index.max(0) as usize) else {
                return;
            };

            let changed = ui.get_selected_index() != index;
            ui.set_selected_index(index);
            ui.set_selected_port_name(entry.name.clone());

            if changed && !ui.get_busy() {
                clear_result(&ui);
                ui.set_status_text(format!("已选择 {}，点击「一键检测」开始", entry.name).into());
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_detect(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_busy() {
            return;
        }

        let ports = ui.get_ports();
        let index = ui.get_selected_index().max(0) as usize;
        let Some(entry) = ports.row_data(index) else {
            ui.set_status_text("请先选择一个串口".into());
            return;
        };
        let port_name = entry.name.to_string();

        ui.set_busy(true);
        ui.set_status_text("正在进入下载模式并读取芯片信息，请勿拔出设备…".into());
        ui.set_verdict_kind("busy".into());
        ui.set_verdict_title("检测中".into());
        ui.set_verdict_reason(format!("正在检测 {port_name} …").into());

        let weak = ui.as_weak();
        thread::spawn(move || {
            let started = Instant::now();
            let result = detector::check_port(&port_name);
            let elapsed = started.elapsed();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    apply_result(&ui, result, elapsed);
                }
            });
        });
    });

    ui.run()
}

fn refresh_ports(ui: &AppWindow) {
    let previous = ui.get_selected_port_name().to_string();
    let items: Vec<PortEntry> = detector::scan_ports()
        .into_iter()
        .map(|port| PortEntry {
            name: port.name.into(),
            description: port.description.into(),
        })
        .collect();

    let count = items.len();
    let selected = items
        .iter()
        .position(|item| item.name.as_str() == previous)
        .unwrap_or(0);

    ui.set_ports(ModelRc::new(VecModel::from(items)));
    ui.set_selected_index(selected as i32);
    clear_result(ui);

    if count == 0 {
        ui.set_selected_port_name("未选择串口".into());
        ui.set_status_text("未发现串口，请检查 USB 连接后点击刷新".into());
        return;
    }

    let name = ui
        .get_ports()
        .row_data(selected)
        .map(|item| item.name.to_string())
        .unwrap_or_default();

    ui.set_selected_port_name(name.clone().into());
    ui.set_status_text(format!("发现 {count} 个串口，已选择 {name}").into());
}

fn apply_result(ui: &AppWindow, result: Result<detector::BoardReport, String>, elapsed: Duration) {
    ui.set_busy(false);

    match result {
        Ok(report) => {
            ui.set_chip_text(report.chip.into());
            ui.set_revision_text(report.revision.into());
            ui.set_mac_text(report.mac.into());
            ui.set_flash_text(report.flash.into());
            ui.set_crystal_text(report.crystal.into());
            ui.set_features_text(report.features.into());
            ui.set_verdict_kind((if report.usable { "ok" } else { "bad" }).into());
            ui.set_verdict_title(
                (if report.usable {
                    "板子可用"
                } else {
                    "板子不可用"
                })
                .into(),
            );
            ui.set_verdict_reason(report.verdict_reason.into());
            ui.set_status_text(format!("检测完成 · 用时 {} ms", elapsed.as_millis()).into());
        }
        Err(error) => {
            clear_result(ui);
            ui.set_verdict_kind("bad".into());
            ui.set_verdict_title("检测失败".into());
            ui.set_verdict_reason(error.clone().into());
            ui.set_status_text(format!("检测失败 · 用时 {} ms", elapsed.as_millis()).into());
        }
    }
}

fn clear_result(ui: &AppWindow) {
    ui.set_chip_text("—".into());
    ui.set_revision_text("—".into());
    ui.set_mac_text("—".into());
    ui.set_flash_text("—".into());
    ui.set_crystal_text("—".into());
    ui.set_features_text("—".into());
    ui.set_verdict_kind("idle".into());
    ui.set_verdict_title("未检测".into());
    ui.set_verdict_reason("连接开发板后刷新串口，再点击「一键检测」。".into());
}
