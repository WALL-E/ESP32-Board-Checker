//! 三语文案工具：lang 0 = 简体中文，1 = 繁体中文，2 = English

/// 按语言从三个现成文案中选择
pub fn tr<'a>(lang: u8, zh: &'a str, tw: &'a str, en: &'a str) -> &'a str {
    match lang {
        1 => tw,
        2 => en,
        _ => zh,
    }
}

/// 按语言格式化文案。三种语言的占位参数必须一致，参数只在选中的分支求值。
///
/// 支持位置参数和命名捕获：
/// `trf!(lang, "发现 {count} 个串口", "發現 {count} 個串口", "Found {count} port(s)")`
/// `trf!(lang, "已选择 {}", "已選擇 {}", "Selected {}", name)`
macro_rules! trf {
    ($lang:expr, $zh:literal, $tw:literal, $en:literal $(, $($arg:tt)*)?) => {
        match $lang {
            1 => format!($tw $(, $($arg)*)?),
            2 => format!($en $(, $($arg)*)?),
            _ => format!($zh $(, $($arg)*)?),
        }
    };
}

pub(crate) use trf;
