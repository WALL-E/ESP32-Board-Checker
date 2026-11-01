//! UI 侧协调器：串口刷新、选择、检测与缓存的编排

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use chrono::{FixedOffset, Utc};
use slint::{ComponentHandle, Model};

use crate::detector;
use crate::i18n::{tr, trf, Lang};
use crate::view::{self, ResultSource, TimedReport};
use crate::AppWindow;

/// 检测结果缓存与当前展示结果
struct ReportState {
    /// 键优先用 USB 序列号，区分同一串口上更换的不同板子
    cache: HashMap<String, TimedReport>,
    /// 当前展示的结果及来源，切换语言后按新语言重放
    last: Option<(TimedReport, ResultSource)>,
}

/// 串口扫描与检测的 UI 侧协调器
#[derive(Clone)]
pub struct Controller {
    ui: slint::Weak<AppWindow>,
    state: Arc<Mutex<ReportState>>,
}

impl Controller {
    pub fn new(ui: &AppWindow) -> Self {
        Self {
            ui: ui.as_weak(),
            state: Arc::new(Mutex::new(ReportState {
                cache: HashMap::new(),
                last: None,
            })),
        }
    }

    /// 重新扫描串口
    pub fn refresh_ports(&self) {
        if let Some(ui) = self.ui.upgrade() {
            view::refresh_ports(&ui);
        }
    }

    /// 选择串口：有缓存则直接展示，否则重置为未检测
    pub fn select_port(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else {
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

        let lang = Lang::from_index(ui.get_lang());
        let key = detector::cache_key(entry.name.as_str());
        let cached = {
            let mut guard = self.state.lock().unwrap();
            let cached = guard.cache.get(&key).cloned();
            guard.last = cached
                .as_ref()
                .map(|report| (report.clone(), ResultSource::Cached));
            cached
        };

        match cached {
            Some(timed) => view::apply_result(&ui, &timed, ResultSource::Cached, lang),
            None => {
                view::clear_result(&ui, lang);
                view::set_status(
                    &ui,
                    trf!(
                        lang,
                        "已选择 {}，点击右侧按钮开始检测",
                        "已選擇 {}，點擊右側按鈕開始檢測",
                        "Selected {} — click the button to start",
                        entry.name
                    ),
                );
            }
        }
    }

    /// 一键检测：始终实时重测，成功后更新缓存
    pub fn detect(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if ui.get_busy() {
            return;
        }
        let lang = Lang::from_index(ui.get_lang());

        let index = ui.get_selected_index().max(0) as usize;
        let Some(entry) = ui.get_ports().row_data(index) else {
            view::set_status(
                &ui,
                tr(
                    lang,
                    "请先选择一个串口",
                    "請先選擇一個串口",
                    "Select a port first.",
                )
                .to_string(),
            );
            return;
        };
        let port_name = entry.name.to_string();

        view::show_checking(&ui, &port_name, lang);

        let key = detector::cache_key(&port_name);
        let ui_weak = self.ui.clone();
        let state = Arc::clone(&self.state);
        thread::spawn(move || {
            let started = Instant::now();
            let result = detector::check_port(&port_name, lang);
            let source = ResultSource::Fresh {
                elapsed: started.elapsed(),
            };

            let _ = slint::invoke_from_event_loop(move || {
                let timed = TimedReport {
                    result,
                    checked_at: beijing_now(),
                };
                {
                    let mut guard = state.lock().unwrap();
                    // 只有成功的检测才进缓存
                    if timed.result.is_ok() {
                        guard.cache.insert(key, timed.clone());
                    }
                    guard.last = Some((timed.clone(), source));
                }
                if let Some(ui) = ui_weak.upgrade() {
                    view::apply_result(&ui, &timed, source, lang);
                }
            });
        });
    }

    /// 切换语言后刷新串口列表，并用新语言重放当前结果
    pub fn apply_language(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let lang = Lang::from_index(ui.get_lang());
        view::refresh_ports(&ui);

        let last = self.state.lock().unwrap().last.clone();
        if let Some((timed, source)) = last {
            view::apply_result(&ui, &timed, source, lang);
        }
    }
}

/// 当前北京时间（UTC+8）
fn beijing_now() -> String {
    let tz = FixedOffset::east_opt(8 * 3600).expect("UTC+8 is a valid offset");
    Utc::now()
        .with_timezone(&tz)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}
