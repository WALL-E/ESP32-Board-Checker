#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod detector;
mod i18n;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{FixedOffset, Utc};
use i18n::{tr, trf};
use slint::{Model, ModelRc, VecModel};

/// 一次检测结果及其完成时间（北京时间）。既作缓存条目，也用于切换语言后重放
#[derive(Clone)]
struct TimedReport {
    result: Result<detector::BoardReport, String>,
    checked_at: String,
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    refresh_ports(&ui);

    // 检测缓存：键优先用 USB 序列号，区分同一串口上更换的不同板子
    let cache: Arc<Mutex<HashMap<String, TimedReport>>> = Arc::new(Mutex::new(HashMap::new()));
    // 当前展示的结果，切换语言后按新语言重放
    let last_display: Arc<Mutex<Option<TimedReport>>> = Arc::new(Mutex::new(None));

    let weak = ui.as_weak();
    ui.on_refresh_ports(move || {
        if let Some(ui) = weak.upgrade() {
            refresh_ports(&ui);
        }
    });

    let weak = ui.as_weak();
    let cache_for_select = Arc::clone(&cache);
    let last_for_select = Arc::clone(&last_display);
    ui.on_select_port(move |index| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let Some(entry) = ui.get_ports().row_data(index.max(0) as usize) else {
            return;
        };

        ui.set_selected_index(index);
        ui.set_selected_port_name(entry.name.clone());
        if ui.get_busy() {
            return;
        }

        let lang = ui.get_lang() as u8;
        let key = detector::cache_key(entry.name.as_str());
        let cached = cache_for_select.lock().unwrap().get(&key).cloned();
        match cached {
            // 已检测过：直接展示缓存结果
            Some(timed) => {
                *last_for_select.lock().unwrap() = Some(timed.clone());
                apply_result(&ui, timed, Duration::ZERO, true, lang);
            }
            None => {
                *last_for_select.lock().unwrap() = None;
                clear_result(&ui, lang);
                ui.set_status_text(
                    trf!(
                        lang,
                        "已选择 {}，点击右侧按钮开始检测",
                        "已選擇 {}，點擊右側按鈕開始檢測",
                        "Selected {} — click the button to start",
                        entry.name
                    )
                    .into(),
                );
            }
        }
    });

    let weak = ui.as_weak();
    let last_for_detect = Arc::clone(&last_display);
    ui.on_detect(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_busy() {
            return;
        }
        let lang = ui.get_lang() as u8;

        let index = ui.get_selected_index().max(0) as usize;
        let Some(entry) = ui.get_ports().row_data(index) else {
            ui.set_status_text(
                tr(lang, "请先选择一个串口", "請先選擇一個串口", "Select a port first.").into(),
            );
            return;
        };
        let port_name = entry.name.to_string();

        ui.set_busy(true);
        ui.set_status_text(
            tr(
                lang,
                "正在进入下载模式并读取芯片信息，请勿拔出设备…",
                "正在進入下載模式並讀取晶片資訊，請勿拔出裝置…",
                "Entering download mode and reading chip info — do not unplug…",
            )
            .into(),
        );
        ui.set_verdict_kind("busy".into());
        ui.set_verdict_title(tr(lang, "检测中", "檢測中", "Checking").into());
        ui.set_verdict_reason(
            trf!(lang, "正在检测 {port_name} …", "正在檢測 {port_name} …", "Checking {port_name}…")
                .into(),
        );

        let key = detector::cache_key(&port_name);
        let weak = ui.as_weak();
        let cache = Arc::clone(&cache);
        let last_for_thread = Arc::clone(&last_for_detect);
        thread::spawn(move || {
            let started = Instant::now();
            let result = detector::check_port(&port_name, lang);
            let elapsed = started.elapsed();

            let _ = slint::invoke_from_event_loop(move || {
                let timed = TimedReport {
                    result,
                    checked_at: beijing_now(),
                };
                // 只有成功的检测才进缓存
                if timed.result.is_ok() {
                    cache.lock().unwrap().insert(key, timed.clone());
                }
                if let Some(ui) = weak.upgrade() {
                    *last_for_thread.lock().unwrap() = Some(timed.clone());
                    apply_result(&ui, timed, elapsed, false, lang);
                }
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_apply_language(move || {
        if let Some(ui) = weak.upgrade() {
            let lang = ui.get_lang() as u8;
            refresh_ports(&ui);
            // 重放当前展示的结果，使其换成新语言
            let last = last_display.lock().unwrap().clone();
            if let Some(timed) = last {
                apply_result(&ui, timed, Duration::ZERO, true, lang);
            }
        }
    });

    ui.run()
}

fn refresh_ports(ui: &AppWindow) {
    let lang = ui.get_lang() as u8;
    let previous = ui.get_selected_port_name().to_string();
    let items: Vec<PortEntry> = detector::scan_ports(lang)
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
    clear_result(ui, lang);

    if count == 0 {
        ui.set_selected_port_name(tr(lang, "未选择串口", "未選擇串口", "No port selected").into());
        ui.set_status_text(
            tr(
                lang,
                "未发现串口，请检查 USB 连接后点击刷新",
                "未發現串口，請檢查 USB 連接後點擊重新整理",
                "No ports found. Check the USB connection and click Refresh.",
            )
            .into(),
        );
        return;
    }

    let name = ui
        .get_ports()
        .row_data(selected)
        .map(|item| item.name.to_string())
        .unwrap_or_default();

    ui.set_selected_port_name(name.clone().into());
    ui.set_status_text(
        trf!(
            lang,
            "发现 {count} 个串口，已选择 {name}",
            "發現 {count} 個串口，已選擇 {name}",
            "Found {count} port(s), selected {name}"
        )
        .into(),
    );
}

/// 把一次检测（或缓存）结果渲染到界面。from_cache 仅影响状态栏措辞
fn apply_result(ui: &AppWindow, timed: TimedReport, elapsed: Duration, from_cache: bool, lang: u8) {
    ui.set_busy(false);

    match timed.result {
        Ok(report) => {
            ui.set_chip_text(report.chip.into());
            ui.set_revision_text(report.revision.into());
            ui.set_mac_text(report.mac.into());
            ui.set_flash_text(report.flash.into());
            ui.set_crystal_text(report.crystal.into());
            ui.set_features_text(report.features.into());
            ui.set_usb_bridge_text(report.usb_bridge.into());
            ui.set_last_check_text(timed.checked_at.clone().into());
            ui.set_verdict_kind((if report.usable { "ok" } else { "bad" }).into());
            ui.set_verdict_title(
                if report.usable {
                    tr(lang, "板子可用", "板子可用", "Board OK")
                } else {
                    tr(lang, "板子不可用", "板子不可用", "Board Unusable")
                }
                .into(),
            );
            ui.set_verdict_reason(report.verdict_reason.into());
            let status = if from_cache {
                trf!(
                    lang,
                    "上次检测 {}，可点击按钮重新检测",
                    "上次檢測 {}，可點擊按鈕重新檢測",
                    "Last checked {} — click the button to re-run",
                    timed.checked_at
                )
            } else {
                trf!(
                    lang,
                    "检测完成 · 用时 {} ms",
                    "檢測完成 · 用時 {} ms",
                    "Check finished in {} ms",
                    elapsed.as_millis()
                )
            };
            ui.set_status_text(status.into());
        }
        Err(error) => {
            clear_result(ui, lang);
            ui.set_verdict_kind("bad".into());
            ui.set_verdict_title(tr(lang, "检测失败", "檢測失敗", "Check Failed").into());
            ui.set_verdict_reason(error.into());
            ui.set_status_text(
                trf!(
                    lang,
                    "检测失败 · 用时 {} ms",
                    "檢測失敗 · 用時 {} ms",
                    "Check failed after {} ms",
                    elapsed.as_millis()
                )
                .into(),
            );
        }
    }
}

fn clear_result(ui: &AppWindow, lang: u8) {
    ui.set_chip_text("—".into());
    ui.set_revision_text("—".into());
    ui.set_mac_text("—".into());
    ui.set_flash_text("—".into());
    ui.set_crystal_text("—".into());
    ui.set_features_text("—".into());
    ui.set_usb_bridge_text("—".into());
    ui.set_last_check_text("—".into());
    ui.set_verdict_kind("idle".into());
    ui.set_verdict_title(tr(lang, "未检测", "未檢測", "Not Checked").into());
    ui.set_verdict_reason(
        tr(
            lang,
            "连接开发板后刷新串口，再点击按钮开始检测。",
            "連接開發板後重新整理串口，再點擊按鈕開始檢測。",
            "Connect the board, refresh ports, then click the button to start.",
        )
        .into(),
    );
}

/// 当前北京时间（UTC+8）
fn beijing_now() -> String {
    let tz = FixedOffset::east_opt(8 * 3600).expect("UTC+8 is a valid offset");
    Utc::now()
        .with_timezone(&tz)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}
