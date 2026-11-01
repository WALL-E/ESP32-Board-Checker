use std::time::Duration;

use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
use espflash::flasher::{FlashSize, Flasher};
use espflash::target::Chip;
use serialport::{FlowControl, SerialPortInfo, SerialPortType, UsbPortInfo};

use crate::i18n::{tr, trf};

/// 一个可选的串口
pub struct PortItem {
    pub name: String,
    pub description: String,
}

/// 一次检测得到的板子信息
#[derive(Clone)]
pub struct BoardReport {
    pub chip: String,
    pub revision: String,
    pub mac: String,
    pub flash: String,
    pub crystal: String,
    pub features: String,
    pub usb_bridge: String,
    pub usable: bool,
    pub verdict_reason: String,
}

/// 扫描系统串口，并把桥接芯片信息作为描述展示
pub fn scan_ports(lang: u8) -> Vec<PortItem> {
    let mut ports = serialport::available_ports().unwrap_or_default();
    ports.sort_by_key(|info| port_sort_key(&info.port_name));

    ports
        .iter()
        .map(|info| PortItem {
            name: info.port_name.clone(),
            description: describe(&info, lang),
        })
        .collect()
}

/// 一键检测：进入下载模式 -> 识别芯片 -> 读取 MAC / Flash 等信息 -> 复位回正常模式
pub fn check_port(port_name: &str, lang: u8) -> Result<BoardReport, String> {
    let usb_info = usb_info_for(port_name);
    let usb_bridge = bridge_text(&usb_info, lang);

    let port = serialport::new(port_name, 115_200)
        .flow_control(FlowControl::None)
        .timeout(Duration::from_secs(3))
        .open_native()
        .map_err(|e| {
            trf!(
                lang,
                "无法打开串口 {port_name}：{e}",
                "無法打開串口 {port_name}：{e}",
                "Cannot open port {port_name}: {e}"
            )
        })?;

    let connection = Connection::new(
        port,
        usb_info,
        ResetAfterOperation::HardReset,
        ResetBeforeOperation::DefaultReset,
        115_200,
    );

    // use_stub = true：加载 RAM stub，Flash 容量自动识别更可靠
    let mut flasher = match Flasher::try_connect(connection, true, false, false, None, None) {
        Ok(flasher) => flasher,
        Err(pair) => {
            let (error, mut connection) = *pair;
            // 连接失败时也尝试复位，避免板子停在下载模式
            let _ = connection.reset();
            return Err(trf!(
                lang,
                "未检测到 ESP 芯片（无法进入下载模式）：{error}",
                "未檢測到 ESP 晶片（無法進入下載模式）：{error}",
                "No ESP chip detected (cannot enter download mode): {error}"
            ));
        }
    };

    let chip = flasher.chip();
    let device = flasher.device_info();
    // 检测完成后复位，让板子回到正常运行模式
    let _ = flasher.connection().reset_after(true, chip);

    let device = device.map_err(|e| {
        trf!(
            lang,
            "芯片已连接，但读取设备信息失败：{e}",
            "晶片已連接，但讀取裝置資訊失敗：{e}",
            "Chip connected, but failed to read device info: {e}"
        )
    })?;

    let flash = flash_text(device.flash_size, lang);
    let (usable, verdict_reason) = judge(&device.mac_address, &flash, device.flash_size.size(), lang);

    let features = if device.features.is_empty() {
        tr(lang, "未报告", "未報告", "Not reported").to_string()
    } else {
        device.features.join(" · ")
    };

    Ok(BoardReport {
        chip: chip_name(device.chip),
        revision: match device.revision {
            Some((major, minor)) => format!("v{major}.{minor}"),
            None => tr(lang, "未知", "未知", "Unknown").to_string(),
        },
        mac: device
            .mac_address
            .unwrap_or_else(|| tr(lang, "未获取", "未獲取", "N/A").to_string()),
        flash,
        crystal: device.crystal_frequency.to_string(),
        features,
        usb_bridge,
        usable,
        verdict_reason,
    })
}

/// 判定板子是否可用，并给出理由
fn judge(mac: &Option<String>, flash: &str, flash_bytes: u32, lang: u8) -> (bool, String) {
    let Some(mac) = mac else {
        return (
            false,
            tr(
                lang,
                "未读取到 MAC 地址（芯片可能处于安全下载模式），无法确认板子状态。",
                "未讀取到 MAC 位址（晶片可能處於安全下載模式），無法確認板子狀態。",
                "MAC address not read (chip may be in secure download mode); board status cannot be confirmed.",
            )
            .to_string(),
        );
    };

    if !mac_is_valid(mac) {
        return (
            false,
            trf!(
                lang,
                "MAC 地址异常（{mac}）：全 0 / 全 F 通常意味着芯片损坏或仿制芯片。",
                "MAC 位址異常（{mac}）：全 0 / 全 F 通常表示晶片損壞或仿製晶片。",
                "Abnormal MAC address ({mac}): all-zero or all-F usually indicates a damaged or counterfeit chip."
            ),
        );
    }

    if flash_bytes < 1024 * 1024 {
        return (
            false,
            trf!(
                lang,
                "Flash 容量仅 {flash}，低于常见应用的最低要求（1 MB）。",
                "Flash 容量僅 {flash}，低於常見應用的最低要求（1 MB）。",
                "Flash size is only {flash}, below the 1 MB minimum for typical applications."
            ),
        );
    }

    (
        true,
        tr(
            lang,
            "芯片响应正常，Flash 与 MAC 均可读取，可正常烧录与运行。",
            "晶片回應正常，Flash 與 MAC 均可讀取，可正常燒錄與運行。",
            "Chip responds normally; Flash and MAC are readable. The board is ready for flashing and operation.",
        )
        .to_string(),
    )
}

fn chip_name(chip: Chip) -> String {
    match chip {
        Chip::Esp32 => "ESP32",
        Chip::Esp32c2 => "ESP32-C2",
        Chip::Esp32c3 => "ESP32-C3",
        Chip::Esp32c5 => "ESP32-C5",
        Chip::Esp32c6 => "ESP32-C6",
        Chip::Esp32c61 => "ESP32-C61",
        Chip::Esp32h2 => "ESP32-H2",
        Chip::Esp32h4 => "ESP32-H4",
        Chip::Esp32p4 => "ESP32-P4",
        Chip::Esp32s2 => "ESP32-S2",
        Chip::Esp32s3 => "ESP32-S3",
        Chip::Esp32s31 => "ESP32-S31",
        _ => return chip.to_string(),
    }
    .to_string()
}

fn flash_text(size: FlashSize, lang: u8) -> String {
    let bytes = size.size();
    if bytes >= 1024 * 1024 {
        trf!(
            lang,
            "{} MB（{} 字节）",
            "{} MB（{} 位元組）",
            "{} MB ({} bytes)",
            bytes / (1024 * 1024),
            bytes
        )
    } else {
        trf!(
            lang,
            "{} KB（{} 字节）",
            "{} KB（{} 位元組）",
            "{} KB ({} bytes)",
            bytes / 1024,
            bytes
        )
    }
}

/// 全 0 / 全 F 的 MAC 通常意味着芯片损坏或仿制芯片
fn mac_is_valid(mac: &str) -> bool {
    let digits: String = mac
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase();

    digits.len() == 12 && digits != "ffffffffffff" && digits != "000000000000"
}

fn describe(info: &SerialPortInfo, lang: u8) -> String {
    match &info.port_type {
        // 驱动上报的 product 字符串通常是英文，改用本地化的桥接芯片名
        SerialPortType::UsbPort(usb) => bridge_text(usb, lang),
        SerialPortType::BluetoothPort => tr(lang, "蓝牙串口", "藍牙串口", "Bluetooth").to_string(),
        SerialPortType::PciPort => tr(lang, "PCI 串口", "PCI 串口", "PCI").to_string(),
        _ => tr(lang, "未知串口设备", "未知串口裝置", "Unknown Device").to_string(),
    }
}

/// 根据 USB VID/PID 推断桥接芯片，如 "CP210x (10C4:EA60)"
fn bridge_text(info: &UsbPortInfo, lang: u8) -> String {
    if info.vid == 0 {
        return tr(lang, "非 USB 串口", "非 USB 串口", "Not a USB port").to_string();
    }

    let name = match info.vid {
        0x303A => tr(lang, "乐鑫原生 USB", "樂鑫原生 USB", "Espressif native USB").to_string(),
        0x1A86 => "CH340/CH341".to_string(),
        0x10C4 => "CP210x".to_string(),
        0x0403 => "FTDI".to_string(),
        0x067B => "PL2303".to_string(),
        _ => info.manufacturer.clone().unwrap_or_else(|| {
            tr(lang, "未知桥接芯片", "未知橋接晶片", "Unknown bridge").to_string()
        }),
    };

    format!("{} ({:04X}:{:04X})", name, info.vid, info.pid)
}

fn usb_info_for(port_name: &str) -> UsbPortInfo {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .find(|info| info.port_name == port_name)
        .and_then(|info| match info.port_type {
            SerialPortType::UsbPort(usb) => Some(usb),
            _ => None,
        })
        .unwrap_or(UsbPortInfo {
            vid: 0,
            pid: 0,
            serial_number: None,
            manufacturer: None,
            product: None,
        })
}

/// 缓存键：优先用 USB 序列号，区分同一串口上更换的不同板子
pub fn cache_key(port_name: &str) -> String {
    let info = usb_info_for(port_name);
    match info.serial_number {
        Some(serial) if !serial.is_empty() => {
            format!("usb:{:04X}:{:04X}:{serial}", info.vid, info.pid)
        }
        _ => port_name.to_string(),
    }
}

/// 让 COM2 排在 COM10 前面
fn port_sort_key(name: &str) -> (String, u32) {
    let digit_start = name
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(index, _)| index)
        .unwrap_or(name.len());

    let (prefix, digits) = name.split_at(digit_start);
    (prefix.to_string(), digits.parse().unwrap_or(0))
}
