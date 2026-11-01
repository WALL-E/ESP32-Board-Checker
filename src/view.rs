//! 界面渲染层：把扫描结果和检测结果写入 Slint 组件

use std::time::Duration;

use slint::{ModelRc, VecModel};

use crate::detector::{self, BoardReport};
use crate::i18n::{tr, trf, Lang};
use crate::{AppWindow, PortEntry};

/// 清空设备信息时显示的占位符
const PLACEHOLDER: &str = "—";

/// 一次检测结果及其完成时间（北京时间）。既作缓存条目，也用于切换语言后重放
#[derive(Clone)]
pub(crate) struct TimedReport {
    pub(crate) result: Result<BoardReport, String>,
    pub(crate) checked_at: String,
}

/// 结果的来源，决定状态栏措辞
#[derive(Clone, Copy)]
pub(crate) enum ResultSource {
    /// 刚刚完成检测
    Fresh { elapsed: Duration },
    /// 直接展示缓存结果
    Cached,
}

impl From<detector::PortItem> for PortEntry {
    fn from(port: detector::PortItem) -> Self {
        Self {
            name: port.name.into(),
            description: port.description.into(),
        }
    }
}

/// 重新扫描串口列表，尽量保留当前选择，并清空旧结果
pub fn refresh_ports(ui: &AppWindow) {
    let lang = Lang::from_index(ui.get_lang());
    let previous = ui.get_selected_port_name().to_string();
    let items: Vec<PortEntry> = detector::scan_ports(lang)
        .into_iter()
        .map(PortEntry::from)
        .collect();

    let count = items.len();
    let selected = items
        .iter()
        .position(|item| item.name.as_str() == previous)
        .unwrap_or(0);
    let name = items
        .get(selected)
        .map(|item| item.name.to_string())
        .unwrap_or_default();

    ui.set_ports(ModelRc::new(VecModel::from(items)));
    ui.set_selected_index(selected as i32);
    clear_result(ui, lang);

    if count == 0 {
        ui.set_selected_port_name(tr(lang, "未选择串口", "未選擇串口", "No port selected").into());
        set_status(
            ui,
            tr(
                lang,
                "未发现串口，请检查 USB 连接后点击刷新",
                "未發現串口，請檢查 USB 連接後點擊重新整理",
                "No ports found. Check the USB connection and click Refresh.",
            )
            .to_string(),
        );
        return;
    }

    ui.set_selected_port_name(name.clone().into());
    set_status(
        ui,
        trf!(
            lang,
            "发现 {count} 个串口，已选择 {name}",
            "發現 {count} 個串口，已選擇 {name}",
            "Found {count} port(s), selected {name}"
        ),
    );
}

/// 切换界面到「检测中」状态
pub fn show_checking(ui: &AppWindow, port_name: &str, lang: Lang) {
    ui.set_busy(true);
    set_status(
        ui,
        tr(
            lang,
            "正在进入下载模式并读取芯片信息，请勿拔出设备…",
            "正在進入下載模式並讀取晶片資訊，請勿拔出裝置…",
            "Entering download mode and reading chip info — do not unplug…",
        )
        .to_string(),
    );
    ui.set_verdict_kind("busy".into());
    ui.set_verdict_title(tr(lang, "检测中", "檢測中", "Checking").into());
    ui.set_verdict_reason(
        trf!(
            lang,
            "正在检测 {port_name} …",
            "正在檢測 {port_name} …",
            "Checking {port_name}…"
        )
        .into(),
    );
}

/// 把一次检测（或缓存）结果渲染到界面
pub fn apply_result(ui: &AppWindow, timed: &TimedReport, source: ResultSource, lang: Lang) {
    ui.set_busy(false);

    let TimedReport { result, checked_at } = timed;
    match result {
        Ok(report) => {
            set_device_fields(ui, report);
            ui.set_last_check_text(checked_at.as_str().into());

            let (kind, title) = verdict_summary(report.usable, lang);
            ui.set_verdict_kind(kind.into());
            ui.set_verdict_title(title.into());
            ui.set_verdict_reason(report.verdict_reason.as_str().into());

            let status = match source {
                ResultSource::Cached => trf!(
                    lang,
                    "上次检测 {}，可点击按钮重新检测",
                    "上次檢測 {}，可點擊按鈕重新檢測",
                    "Last checked {} — click the button to re-run",
                    checked_at
                ),
                ResultSource::Fresh { elapsed } => trf!(
                    lang,
                    "检测完成 · 用时 {} ms",
                    "檢測完成 · 用時 {} ms",
                    "Check finished in {} ms",
                    elapsed.as_millis()
                ),
            };
            set_status(ui, status);
        }
        Err(error) => {
            clear_result(ui, lang);
            ui.set_verdict_kind("bad".into());
            ui.set_verdict_title(tr(lang, "检测失败", "檢測失敗", "Check Failed").into());
            ui.set_verdict_reason(error.as_str().into());

            let elapsed = match source {
                ResultSource::Fresh { elapsed } => elapsed,
                // 检测失败不会写入缓存，Cached 仅作兜底
                ResultSource::Cached => Duration::ZERO,
            };
            set_status(
                ui,
                trf!(
                    lang,
                    "检测失败 · 用时 {} ms",
                    "檢測失敗 · 用時 {} ms",
                    "Check failed after {} ms",
                    elapsed.as_millis()
                ),
            );
        }
    }
}

/// 重置设备信息与判定卡
pub fn clear_result(ui: &AppWindow, lang: Lang) {
    ui.set_chip_text(PLACEHOLDER.into());
    ui.set_revision_text(PLACEHOLDER.into());
    ui.set_mac_text(PLACEHOLDER.into());
    ui.set_flash_text(PLACEHOLDER.into());
    ui.set_crystal_text(PLACEHOLDER.into());
    ui.set_features_text(PLACEHOLDER.into());
    ui.set_usb_bridge_text(PLACEHOLDER.into());
    ui.set_last_check_text(PLACEHOLDER.into());
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

pub fn set_status(ui: &AppWindow, text: String) {
    ui.set_status_text(text.into());
}

fn set_device_fields(ui: &AppWindow, report: &BoardReport) {
    ui.set_chip_text(report.chip.as_str().into());
    ui.set_revision_text(report.revision.as_str().into());
    ui.set_mac_text(report.mac.as_str().into());
    ui.set_flash_text(report.flash.as_str().into());
    ui.set_crystal_text(report.crystal.as_str().into());
    ui.set_features_text(report.features.as_str().into());
    ui.set_usb_bridge_text(report.usb_bridge.as_str().into());
}

fn verdict_summary(usable: bool, lang: Lang) -> (&'static str, &'static str) {
    if usable {
        ("ok", tr(lang, "板子可用", "板子可用", "Board OK"))
    } else {
        ("bad", tr(lang, "板子不可用", "板子不可用", "Board Unusable"))
    }
}
