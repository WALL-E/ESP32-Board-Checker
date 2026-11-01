//! 三语文案工具：用 Lang 枚举替代裸数字，避免调用方传错语言索引

/// 界面语言，索引与 Slint 的 `lang` 属性一致
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Simplified,
    Traditional,
    English,
}

impl Lang {
    /// 由 Slint 的 `lang` 整型属性转换
    pub const fn from_index(index: i32) -> Self {
        match index {
            1 => Self::Traditional,
            2 => Self::English,
            _ => Self::Simplified,
        }
    }
}

/// 按语言从三个现成文案中选择
pub fn tr<'a>(lang: Lang, zh: &'a str, tw: &'a str, en: &'a str) -> &'a str {
    match lang {
        Lang::Traditional => tw,
        Lang::English => en,
        Lang::Simplified => zh,
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
            $crate::i18n::Lang::Traditional => format!($tw $(, $($arg)*)?),
            $crate::i18n::Lang::English => format!($en $(, $($arg)*)?),
            $crate::i18n::Lang::Simplified => format!($zh $(, $($arg)*)?),
        }
    };
}

pub(crate) use trf;
