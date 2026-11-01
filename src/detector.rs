use std::time::Duration;

use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
use espflash::flasher::{FlashSize, Flasher};
use espflash::target::Chip;
use serialport::{FlowControl, SerialPortInfo, SerialPortType, UsbPortInfo};

/// 一个可选的串口
pub struct PortItem {
    pub name: String,
    pub description: String,
}

/// 一次检测得到的板子信息
pub struct BoardReport {
    pub chip: String,
    pub revision: String,
    pub mac: String,
    pub flash: String,
    pub crystal: String,
    pub features: String,
    pub usable: bool,
    pub verdict_reason: String,
}

/// 扫描系统串口，并把 USB 设备的信息作为描述展示
pub fn scan_ports() -> Vec<PortItem> {
    let mut ports = serialport::available_ports().unwrap_or_default();
    ports.sort_by_key(|info| port_sort_key(&info.port_name));

    ports
        .iter()
        .map(|info| PortItem {
            name: info.port_name.clone(),
            description: describe(&info),
        })
        .collect()
}

/// 一键检测：进入下载模式 -> 识别芯片 -> 读取 MAC / Flash 等信息 -> 复位回正常模式
pub fn check_port(port_name: &str) -> Result<BoardReport, String> {
    let usb_info = usb_info_for(port_name);

    let port = serialport::new(port_name, 115_200)
        .flow_control(FlowControl::None)
        .timeout(Duration::from_secs(3))
        .open_native()
        .map_err(|e| format!("无法打开串口 {port_name}：{e}"))?;

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
            return Err(format!("未检测到 ESP 芯片（无法进入下载模式）：{error}"));
        }
    };

    let chip = flasher.chip();
    let device = flasher.device_info();
    // 检测完成后复位，让板子回到正常运行模式
    let _ = flasher.connection().reset_after(true, chip);

    let device = device.map_err(|e| format!("芯片已连接，但读取设备信息失败：{e}"))?;

    let mac = device.mac_address.clone();
    let flash = flash_text(device.flash_size);
    let flash_bytes = device.flash_size.size();
    let mac_valid = mac.as_deref().map(mac_is_valid).unwrap_or(false);

    let (usable, verdict_reason) = if mac.is_none() {
        (
            false,
            "未读取到 MAC 地址（芯片可能处于安全下载模式），无法确认板子状态。".to_string(),
        )
    } else if !mac_valid {
        (
            false,
            format!(
                "MAC 地址异常（{}）：全 0 / 全 F 通常意味着芯片损坏或仿制芯片。",
                mac.clone().unwrap_or_default()
            ),
        )
    } else if flash_bytes < 1024 * 1024 {
        (
            false,
            format!("Flash 容量仅 {flash}，低于常见应用的最低要求（1 MB）。"),
        )
    } else {
        (
            true,
            "芯片响应正常，Flash 与 MAC 均可读取，可正常烧录与运行。".to_string(),
        )
    };

    let features = if device.features.is_empty() {
        "未报告".to_string()
    } else {
        device.features.join(" · ")
    };

    Ok(BoardReport {
        chip: chip_name(device.chip),
        revision: match device.revision {
            Some((major, minor)) => format!("v{major}.{minor}"),
            None => "未知".to_string(),
        },
        mac: mac.unwrap_or_else(|| "未获取".to_string()),
        flash,
        crystal: device.crystal_frequency.to_string(),
        features,
        usable,
        verdict_reason,
    })
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

fn flash_text(size: FlashSize) -> String {
    let bytes = size.size();
    if bytes >= 1024 * 1024 {
        format!("{} MB（{} 字节）", bytes / (1024 * 1024), bytes)
    } else {
        format!("{} KB（{} 字节）", bytes / 1024, bytes)
    }
}

fn mac_is_valid(mac: &str) -> bool {
    let digits: String = mac
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase();

    digits.len() == 12 && digits != "ffffffffffff" && digits != "000000000000"
}

fn describe(info: &SerialPortInfo) -> String {
    match &info.port_type {
        SerialPortType::UsbPort(usb) => {
            let label = usb
                .product
                .clone()
                .or_else(|| usb.manufacturer.clone())
                .unwrap_or_else(|| "USB 串口设备".to_string());
            format!("{label} · VID:PID {:04X}:{:04X}", usb.vid, usb.pid)
        }
        SerialPortType::BluetoothPort => "蓝牙串口".to_string(),
        SerialPortType::PciPort => "PCI 串口".to_string(),
        _ => "未知串口设备".to_string(),
    }
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
