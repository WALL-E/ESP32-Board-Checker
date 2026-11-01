#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod detector;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{FixedOffset, Utc};
use slint::{Model, ModelRc, VecModel};

/// 三语文案：lang 0 = 简体中文，1 = 繁体中文，2 = English
fn tr<'a>(lang: u8, zh: &'a str, tw: &'a str, en: &'a str) -> &'a str {
    match lang {
        1 => tw,
        2 => en,
        _ => zh,
    }
}

/// 当前展示的结果，用于切换语言后按新语言重放
#[derive(Clone)]
struct LastDisplay {
    result: Result<detector::BoardReport, String>,
    checked_at: String,
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    refresh_ports(&ui);

    let cache: Arc<Mutex<HashMap<String, CachedReport>>> = Arc::new(Mutex::new(HashMap::new()));
    let last_display: Arc<Mutex<Option<LastDisplay>>> = Arc::new(Mutex::new(None));

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
        if let Some(ui) = weak.upgrade() {
            let lang = ui.get_lang() as u8;
            let ports = ui.get_ports();
            let Some(entry) = ports.row_data(index.max(0) as usize) else {
                return;
            };

            ui.set_selected_index(index);
            ui.set_selected_port_name(entry.name.clone());

            if !ui.get_busy() {
                let key = detector::cache_key(entry.name.as_str());
                let cached = cache_for_select.lock().unwrap().get(&key).cloned();
                if let Some(cached) = cached {
                    let checked_at = cached.checked_at.clone();
                    *last_for_select.lock().unwrap() = Some(LastDisplay {
                        result: Ok(cached.report.clone()),
                        checked_at: checked_at.clone(),
                    });
                    apply_result(&ui, Ok(cached.report), Duration::ZERO, &checked_at, true, lang);
                } else {
                    *last_for_select.lock().unwrap() = None;
                    clear_result(&ui, lang);
                    let msg = match lang {
                        1 => format!("已選擇 {}，點擊右側按鈕開始檢測", entry.name),
                        2 => format!("Selected {} — click the button to start", entry.name),
                        _ => format!("已选择 {}，点击右侧按钮开始检测", entry.name),
                    };
                    ui.set_status_text(msg.into());
                }
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

        let ports = ui.get_ports();
        let index = ui.get_selected_index().max(0) as usize;
        let Some(entry) = ports.row_data(index) else {
            ui.set_status_text(tr(lang, "请先选择一个串口", "請先選擇一個串口", "Select a port first.").into());
            return;
        };
        let port_name = entry.name.to_string();

        ui.set_busy(true);
        ui.set_status_text(tr(
            lang,
            "正在进入下载模式并读取芯片信息，请勿拔出设备…",
            "正在進入下載模式並讀取晶片資訊，請勿拔出裝置…",
            "Entering download mode and reading chip info — do not unplug…",
        ).into());
        ui.set_verdict_kind("busy".into());
        ui.set_verdict_title(tr(lang, "检测中", "檢測中", "Checking").into());
        let checking_msg = match lang {
            1 => format!("正在檢測 {port_name} …"),
            2 => format!("Checking {port_name}…"),
            _ => format!("正在检测 {port_name} …"),
        };
        ui.set_verdict_reason(checking_msg.into());

        let key = detector::cache_key(&port_name);
        let weak = ui.as_weak();
        let cache = Arc::clone(&cache);
        let last_for_thread = Arc::clone(&last_for_detect);
        thread::spawn(move || {
            let started = Instant::now();
            let result = detector::check_port(&port_name, lang);
            let elapsed = started.elapsed();
            let checked_at = beijing_now();

            let _ = slint::invoke_from_event_loop(move || {
                if let Ok(report) = &result {
                    cache.lock().unwrap().insert(
                        key,
                        CachedReport {
                            report: report.clone(),
                            checked_at: checked_at.clone(),
                        },
                    );
                }
                if let Some(ui) = weak.upgrade() {
                    *last_for_thread.lock().unwrap() = Some(LastDisplay {
                        result: result.clone(),
                        checked_at: checked_at.clone(),
                    });
                    apply_result(&ui, result, elapsed, &checked_at, false, lang);
                }
            });
        });
    });

    let weak = ui.as_weak();
    let last_for_lang = Arc::clone(&last_display);
    ui.on_apply_language(move || {
        if let Some(ui) = weak.upgrade() {
            let lang = ui.get_lang() as u8;
            refresh_ports(&ui);
            // 重放当前展示的结果，使其换成新语言
            let last = last_for_lang.lock().unwrap().clone();
            if let Some(last) = last {
                apply_result(&ui, last.result, Duration::ZERO, &last.checked_at, true, lang);
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
        ui.set_status_text(tr(
            lang,
            "未发现串口，请检查 USB 连接后点击刷新",
            "未發現串口，請檢查 USB 連接後點擊重新整理",
            "No ports found. Check the USB connection and click Refresh.",
        ).into());
        return;
    }

    let name = ui
        .get_ports()
        .row_data(selected)
        .map(|item| item.name.to_string())
        .unwrap_or_default();

    ui.set_selected_port_name(name.clone().into());
    let msg = match lang {
        1 => format!("發現 {count} 個串口，已選擇 {name}"),
        2 => format!("Found {count} port(s), selected {name}"),
        _ => format!("发现 {count} 个串口，已选择 {name}"),
    };
    ui.set_status_text(msg.into());
}

fn apply_result(
    ui: &AppWindow,
    result: Result<detector::BoardReport, String>,
    elapsed: Duration,
    checked_at: &str,
    from_cache: bool,
    lang: u8,
) {
    ui.set_busy(false);

    match result {
        Ok(report) => {
            ui.set_chip_text(report.chip.into());
            ui.set_revision_text(report.revision.into());
            ui.set_mac_text(report.mac.into());
            ui.set_flash_text(report.flash.into());
            ui.set_crystal_text(report.crystal.into());
            ui.set_features_text(report.features.into());
            ui.set_usb_bridge_text(report.usb_bridge.into());
            ui.set_last_check_text(checked_at.into());
            ui.set_verdict_kind((if report.usable { "ok" } else { "bad" }).into());
            ui.set_verdict_title(
                tr(
                    lang,
                    if report.usable { "板子可用" } else { "板子不可用" },
                    if report.usable { "板子可用" } else { "板子不可用" },
                    if report.usable { "Board OK" } else { "Board Unusable" },
                )
                .into(),
            );
            ui.set_verdict_reason(report.verdict_reason.into());
            let status = if from_cache {
                match lang {
                    1 => format!("上次檢測 {checked_at}，可點擊按鈕重新檢測"),
                    2 => format!("Last checked {checked_at} — click the button to re-run"),
                    _ => format!("上次检测 {checked_at}，可点击按钮重新检测"),
                }
            } else {
                match lang {
                    1 => format!("檢測完成 · 用時 {} ms", elapsed.as_millis()),
                    2 => format!("Check finished in {} ms", elapsed.as_millis()),
                    _ => format!("检测完成 · 用时 {} ms", elapsed.as_millis()),
                }
            };
            ui.set_status_text(status.into());
        }
        Err(error) => {
            clear_result(ui, lang);
            ui.set_verdict_kind("bad".into());
            ui.set_verdict_title(tr(lang, "检测失败", "檢測失敗", "Check Failed").into());
            ui.set_verdict_reason(error.clone().into());
            let msg = match lang {
                1 => format!("檢測失敗 · 用時 {} ms", elapsed.as_millis()),
                2 => format!("Check failed after {} ms", elapsed.as_millis()),
                _ => format!("检测失败 · 用时 {} ms", elapsed.as_millis()),
            };
            ui.set_status_text(msg.into());
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
    ui.set_verdict_reason(tr(
        lang,
        "连接开发板后刷新串口，再点击按钮开始检测。",
        "連接開發板後重新整理串口，再點擊按鈕開始檢測。",
        "Connect the board, refresh ports, then click the button to start.",
    ).into());
}

/// 一条缓存的检测结果（成功的检测才会缓存）
#[derive(Clone)]
struct CachedReport {
    report: detector::BoardReport,
    checked_at: String,
}

/// 当前北京时间（UTC+8）
fn beijing_now() -> String {
    let tz = FixedOffset::east_opt(8 * 3600).expect("UTC+8 is a valid offset");
    Utc::now()
        .with_timezone(&tz)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}
