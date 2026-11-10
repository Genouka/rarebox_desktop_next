//! 鸿蒙键码 → GPUI `Keystroke`。
//!
//! OHOS 的键码来自 `multimodalinput/oh_key_code.h`，与 Android 完全不同：
//! 字母从 `KEYCODE_A = 2017` 到 `KEYCODE_Z = 2042`，数字是 `KEYCODE_0 = 2000`
//! 起的连续段，功能键 `KEYCODE_F1 = 2090 … F12 = 2101`，`KEYCODE_ENTER = 2054`、
//! `KEYCODE_DEL = 2055`（退格）、`KEYCODE_FORWARD_DEL = 2071`（Delete）。
//!
//! 另一个与 Android 的重要差异：**OHOS 不提供 `getUnicodeChar` 等价物**
//! （风险 R14）。因此这里不尝试查询系统按键→字符映射：
//!
//! * 快捷键识别只用 `key`（物理按键名）；
//! * 可见字符走 IME 的 `insertText`（`text_input` 模块）；
//! * 硬件键盘没有 IME 时，用 Shift/CapsLock 推断 A–Z、数字行与常见标点的
//!   ASCII 字符（[`key_char_for`]）——这对调试工具里的终端输入已经够用，
//!   并且不会假装知道非美式布局的重映射。
//!
//! 修饰键状态由 [`ModifierState`] 跟踪：XComponent 的 `GetKeyEventModifierKeyStates`
//! 需要 API 20+，而且没有公开的位掩码定义，所以按键事件先用“自己看到的修饰键
//! 按下/抬起”维护状态；NAPI 的 `key(eventJson)` 也允许宿主直接给出掩码覆盖它。

use gpui::{Keystroke, Modifiers, PlatformKeyboardLayout};

// ── 键码（multimodalinput/oh_key_code.h） ────────────────────────────────────

/// `KEY_UNKNOWN`（`native_xcomponent_key_event.h`）。
pub const KEYCODE_UNKNOWN: i32 = -1;

pub const KEYCODE_HOME: i32 = 1;
pub const KEYCODE_BACK: i32 = 2;
pub const KEYCODE_VOLUME_UP: i32 = 16;
pub const KEYCODE_VOLUME_DOWN: i32 = 17;

pub const KEYCODE_0: i32 = 2000;
pub const KEYCODE_1: i32 = 2001;
pub const KEYCODE_2: i32 = 2002;
pub const KEYCODE_3: i32 = 2003;
pub const KEYCODE_4: i32 = 2004;
pub const KEYCODE_5: i32 = 2005;
pub const KEYCODE_6: i32 = 2006;
pub const KEYCODE_7: i32 = 2007;
pub const KEYCODE_8: i32 = 2008;
pub const KEYCODE_9: i32 = 2009;
pub const KEYCODE_STAR: i32 = 2010;
pub const KEYCODE_POUND: i32 = 2011;

pub const KEYCODE_DPAD_UP: i32 = 2012;
pub const KEYCODE_DPAD_DOWN: i32 = 2013;
pub const KEYCODE_DPAD_LEFT: i32 = 2014;
pub const KEYCODE_DPAD_RIGHT: i32 = 2015;
pub const KEYCODE_DPAD_CENTER: i32 = 2016;

pub const KEYCODE_A: i32 = 2017;
/// 最后一个字母键（`KEYCODE_Z`）。
pub const KEYCODE_Z: i32 = 2042;

pub const KEYCODE_COMMA: i32 = 2043;
pub const KEYCODE_PERIOD: i32 = 2044;
pub const KEYCODE_ALT_LEFT: i32 = 2045;
pub const KEYCODE_ALT_RIGHT: i32 = 2046;
pub const KEYCODE_SHIFT_LEFT: i32 = 2047;
pub const KEYCODE_SHIFT_RIGHT: i32 = 2048;
pub const KEYCODE_TAB: i32 = 2049;
pub const KEYCODE_SPACE: i32 = 2050;
pub const KEYCODE_SYM: i32 = 2051;
pub const KEYCODE_EXPLORER: i32 = 2052;
pub const KEYCODE_ENTER: i32 = 2054;
pub const KEYCODE_DEL: i32 = 2055;
pub const KEYCODE_GRAVE: i32 = 2056;
pub const KEYCODE_MINUS: i32 = 2057;
pub const KEYCODE_EQUALS: i32 = 2058;
pub const KEYCODE_LEFT_BRACKET: i32 = 2059;
pub const KEYCODE_RIGHT_BRACKET: i32 = 2060;
pub const KEYCODE_BACKSLASH: i32 = 2061;
pub const KEYCODE_SEMICOLON: i32 = 2062;
pub const KEYCODE_APOSTROPHE: i32 = 2063;
pub const KEYCODE_SLASH: i32 = 2064;
pub const KEYCODE_AT: i32 = 2065;
pub const KEYCODE_MENU: i32 = 2067;
pub const KEYCODE_PAGE_UP: i32 = 2068;
pub const KEYCODE_PAGE_DOWN: i32 = 2069;
pub const KEYCODE_ESCAPE: i32 = 2070;
pub const KEYCODE_FORWARD_DEL: i32 = 2071;
pub const KEYCODE_CTRL_LEFT: i32 = 2072;
pub const KEYCODE_CTRL_RIGHT: i32 = 2073;
pub const KEYCODE_CAPS_LOCK: i32 = 2074;
pub const KEYCODE_SCROLL_LOCK: i32 = 2075;
pub const KEYCODE_META_LEFT: i32 = 2076;
pub const KEYCODE_META_RIGHT: i32 = 2077;
pub const KEYCODE_FUNCTION: i32 = 2078;
pub const KEYCODE_SYSRQ: i32 = 2079;
pub const KEYCODE_MOVE_HOME: i32 = 2081;
pub const KEYCODE_MOVE_END: i32 = 2082;
pub const KEYCODE_INSERT: i32 = 2083;
pub const KEYCODE_F1: i32 = 2090;
pub const KEYCODE_F12: i32 = 2101;
pub const KEYCODE_NUM_LOCK: i32 = 2102;
pub const KEYCODE_NUMPAD_0: i32 = 2103;
pub const KEYCODE_NUMPAD_9: i32 = 2112;
pub const KEYCODE_NUMPAD_DIVIDE: i32 = 2113;
pub const KEYCODE_NUMPAD_MULTIPLY: i32 = 2114;
pub const KEYCODE_NUMPAD_SUBTRACT: i32 = 2115;
pub const KEYCODE_NUMPAD_ADD: i32 = 2116;
pub const KEYCODE_NUMPAD_DOT: i32 = 2117;
pub const KEYCODE_NUMPAD_COMMA: i32 = 2118;
pub const KEYCODE_NUMPAD_ENTER: i32 = 2119;

/// `OH_NATIVEXCOMPONENT_KEY_ACTION_DOWN`。
pub const KEY_ACTION_DOWN: i32 = 0;
/// `OH_NATIVEXCOMPONENT_KEY_ACTION_UP`。
pub const KEY_ACTION_UP: i32 = 1;
/// `OH_NATIVEXCOMPONENT_KEY_ACTION_UNKNOWN`。
pub const KEY_ACTION_UNKNOWN: i32 = -1;

// ── 键码 → GPUI 键名 ─────────────────────────────────────────────────────────

/// 把 OHOS 键码转换成 GPUI 的键名。
///
/// 纯修饰键（Shift/Ctrl/Alt/Meta/CapsLock/…）、音量键与系统键返回 `None`：
/// 它们要么由 [`ModifierState`] 消费，要么不属于应用输入。
pub fn keycode_to_key(code: i32) -> Option<&'static str> {
    // 字母与数字是连续段，先处理。
    if (KEYCODE_A..=KEYCODE_Z).contains(&code) {
        const LETTERS: [&str; 26] = [
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q",
            "r", "s", "t", "u", "v", "w", "x", "y", "z",
        ];
        return Some(LETTERS[(code - KEYCODE_A) as usize]);
    }
    if (KEYCODE_0..=KEYCODE_9).contains(&code) {
        const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
        return Some(DIGITS[(code - KEYCODE_0) as usize]);
    }
    if (KEYCODE_F1..=KEYCODE_F12).contains(&code) {
        const FUNCTION_KEYS: [&str; 12] = [
            "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
        ];
        return Some(FUNCTION_KEYS[(code - KEYCODE_F1) as usize]);
    }
    if (KEYCODE_NUMPAD_0..=KEYCODE_NUMPAD_9).contains(&code) {
        const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
        return Some(DIGITS[(code - KEYCODE_NUMPAD_0) as usize]);
    }

    let key = match code {
        KEYCODE_COMMA => ",",
        KEYCODE_PERIOD | KEYCODE_NUMPAD_DOT => ".",
        KEYCODE_SPACE => " ",
        KEYCODE_GRAVE => "`",
        KEYCODE_MINUS | KEYCODE_NUMPAD_SUBTRACT => "-",
        KEYCODE_EQUALS => "=",
        KEYCODE_LEFT_BRACKET => "[",
        KEYCODE_RIGHT_BRACKET => "]",
        KEYCODE_BACKSLASH => "\\",
        KEYCODE_SEMICOLON => ";",
        KEYCODE_APOSTROPHE => "'",
        KEYCODE_SLASH | KEYCODE_NUMPAD_DIVIDE => "/",
        KEYCODE_AT => "@",
        KEYCODE_STAR | KEYCODE_NUMPAD_MULTIPLY => "*",
        KEYCODE_POUND => "#",
        KEYCODE_NUMPAD_ADD => "+",
        KEYCODE_NUMPAD_COMMA => ",",

        KEYCODE_ENTER | KEYCODE_NUMPAD_ENTER | KEYCODE_DPAD_CENTER => "enter",
        KEYCODE_ESCAPE | KEYCODE_BACK => "escape",
        KEYCODE_DEL => "backspace",
        KEYCODE_FORWARD_DEL => "delete",
        KEYCODE_TAB => "tab",
        KEYCODE_DPAD_UP => "up",
        KEYCODE_DPAD_DOWN => "down",
        KEYCODE_DPAD_LEFT => "left",
        KEYCODE_DPAD_RIGHT => "right",
        KEYCODE_MOVE_HOME => "home",
        KEYCODE_MOVE_END => "end",
        KEYCODE_INSERT => "insert",
        KEYCODE_PAGE_UP => "pageup",
        KEYCODE_PAGE_DOWN => "pagedown",
        KEYCODE_MENU => "menu",

        // 纯修饰键 / 系统键：不产生 Keystroke。
        KEYCODE_SHIFT_LEFT
        | KEYCODE_SHIFT_RIGHT
        | KEYCODE_CTRL_LEFT
        | KEYCODE_CTRL_RIGHT
        | KEYCODE_ALT_LEFT
        | KEYCODE_ALT_RIGHT
        | KEYCODE_META_LEFT
        | KEYCODE_META_RIGHT
        | KEYCODE_FUNCTION
        | KEYCODE_CAPS_LOCK
        | KEYCODE_NUM_LOCK
        | KEYCODE_SCROLL_LOCK
        | KEYCODE_SYSRQ
        | KEYCODE_VOLUME_UP
        | KEYCODE_VOLUME_DOWN
        | KEYCODE_HOME
        | KEYCODE_SYM
        | KEYCODE_EXPLORER
        | KEYCODE_UNKNOWN => return None,

        _ => return None,
    };
    Some(key)
}

/// 该键码是否是修饰键（Shift/Ctrl/Alt/Meta/CapsLock/NumLock/ScrollLock/Fn）。
pub fn is_modifier_key(code: i32) -> bool {
    matches!(
        code,
        KEYCODE_SHIFT_LEFT
            | KEYCODE_SHIFT_RIGHT
            | KEYCODE_CTRL_LEFT
            | KEYCODE_CTRL_RIGHT
            | KEYCODE_ALT_LEFT
            | KEYCODE_ALT_RIGHT
            | KEYCODE_META_LEFT
            | KEYCODE_META_RIGHT
            | KEYCODE_FUNCTION
            | KEYCODE_CAPS_LOCK
            | KEYCODE_NUM_LOCK
            | KEYCODE_SCROLL_LOCK
    )
}

// ── 修饰键状态 ───────────────────────────────────────────────────────────────

/// 由按键流维护的修饰键状态。
///
/// OHOS 没有把 XComponent 的修饰键位掩码公开成常量（`GetKeyEventModifierKeyStates`
/// 是 API 20+，语义未文档化），所以左/右修饰键各自记一个布尔值，再合成
/// GPUI 的 [`Modifiers`]。`caps_lock` 在按下时翻转——与桌面键盘一致。
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ModifierState {
    pub shift_left: bool,
    pub shift_right: bool,
    pub control_left: bool,
    pub control_right: bool,
    pub alt_left: bool,
    pub alt_right: bool,
    pub meta_left: bool,
    pub meta_right: bool,
    pub function: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
}

impl ModifierState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 应用一个按键事件；返回 `true` 表示它是修饰键（调用方不应再把它当字符键）。
    pub fn apply_key(&mut self, code: i32, down: bool) -> bool {
        let slot = match code {
            KEYCODE_SHIFT_LEFT => &mut self.shift_left,
            KEYCODE_SHIFT_RIGHT => &mut self.shift_right,
            KEYCODE_CTRL_LEFT => &mut self.control_left,
            KEYCODE_CTRL_RIGHT => &mut self.control_right,
            KEYCODE_ALT_LEFT => &mut self.alt_left,
            KEYCODE_ALT_RIGHT => &mut self.alt_right,
            KEYCODE_META_LEFT => &mut self.meta_left,
            KEYCODE_META_RIGHT => &mut self.meta_right,
            KEYCODE_FUNCTION => &mut self.function,
            KEYCODE_CAPS_LOCK => {
                if down {
                    self.caps_lock = !self.caps_lock;
                }
                return true;
            }
            KEYCODE_NUM_LOCK => {
                if down {
                    self.num_lock = !self.num_lock;
                }
                return true;
            }
            KEYCODE_SCROLL_LOCK => {
                if down {
                    self.scroll_lock = !self.scroll_lock;
                }
                return true;
            }
            _ => return false,
        };
        *slot = down;
        true
    }

    /// 合成 GPUI 修饰键。`function` 对 Fn 键与 CapsLock 都成立——GPUI 用它
    /// 区分“功能键语义”，与平台快捷键解析一致。
    pub fn modifiers(&self) -> Modifiers {
        Modifiers {
            control: self.control_left || self.control_right,
            alt: self.alt_left || self.alt_right,
            shift: self.shift_left || self.shift_right,
            platform: self.meta_left || self.meta_right,
            function: self.function || self.caps_lock,
        }
    }

    /// 用宿主给出的位掩码覆盖修饰键状态（NAPI `key` 的 `modifiers` 字段）。
    ///
    /// 位定义与 ArkTS 侧的 `KeyEvent.modifierKeyState` 约定一致：
    /// `1 = shift, 2 = ctrl, 4 = alt, 8 = meta`。
    pub fn set_from_mask(&mut self, mask: u32) {
        self.shift_left = mask & 1 != 0;
        self.shift_right = false;
        self.control_left = mask & 2 != 0;
        self.control_right = false;
        self.alt_left = mask & 4 != 0;
        self.alt_right = false;
        self.meta_left = mask & 8 != 0;
        self.meta_right = false;
    }
}

// ── 字符推断 ─────────────────────────────────────────────────────────────────

/// 硬件键盘的可见字符（没有 IME 时使用）。
///
/// OHOS 不提供按键→字符查询（R14），这里只覆盖美式布局的确定性部分：
/// 字母（Shift xor CapsLock → 大写）、数字行与常见标点。返回 `None` 表示
/// 该键不产生可见字符（功能键、方向键等）。
pub fn key_char_for(code: i32, modifiers: Modifiers, caps_lock: bool) -> Option<String> {
    let shifted = modifiers.shift ^ caps_lock;
    if (KEYCODE_A..=KEYCODE_Z).contains(&code) {
        let lower = (b'a' + (code - KEYCODE_A) as u8) as char;
        return Some(if shifted {
            lower.to_ascii_uppercase().to_string()
        } else {
            lower.to_string()
        });
    }

    let ch = match code {
        KEYCODE_0..=KEYCODE_9 => {
            let digit = (b'0' + (code - KEYCODE_0) as u8) as char;
            if modifiers.shift {
                ")!@#$%^&*(".chars().nth((code - KEYCODE_0) as usize)?
            } else {
                digit
            }
        }
        KEYCODE_SPACE => ' ',
        KEYCODE_GRAVE => {
            if modifiers.shift {
                '~'
            } else {
                '`'
            }
        }
        KEYCODE_MINUS => {
            if modifiers.shift {
                '_'
            } else {
                '-'
            }
        }
        KEYCODE_EQUALS => {
            if modifiers.shift {
                '+'
            } else {
                '='
            }
        }
        KEYCODE_LEFT_BRACKET => {
            if modifiers.shift {
                '{'
            } else {
                '['
            }
        }
        KEYCODE_RIGHT_BRACKET => {
            if modifiers.shift {
                '}'
            } else {
                ']'
            }
        }
        KEYCODE_BACKSLASH => {
            if modifiers.shift {
                '|'
            } else {
                '\\'
            }
        }
        KEYCODE_SEMICOLON => {
            if modifiers.shift {
                ':'
            } else {
                ';'
            }
        }
        KEYCODE_APOSTROPHE => {
            if modifiers.shift {
                '"'
            } else {
                '\''
            }
        }
        KEYCODE_COMMA => {
            if modifiers.shift {
                '<'
            } else {
                ','
            }
        }
        KEYCODE_PERIOD => {
            if modifiers.shift {
                '>'
            } else {
                '.'
            }
        }
        KEYCODE_SLASH => {
            if modifiers.shift {
                '?'
            } else {
                '/'
            }
        }
        KEYCODE_TAB => '\t',
        _ => return None,
    };
    Some(ch.to_string())
}

/// 把一次按键事件转成 GPUI [`Keystroke`]。
///
/// `unicode` 来自 NAPI 的 `key(eventJson).unicode`（宿主若能提供）；没有时
/// 用 [`key_char_for`] 推断。修饰键本身返回 `None`。
pub fn key_event_to_keystroke(
    code: i32,
    modifiers: Modifiers,
    caps_lock: bool,
    unicode: Option<char>,
) -> Option<Keystroke> {
    let key = keycode_to_key(code)?;
    let key_char = match unicode {
        Some(ch) if !ch.is_control() => Some(ch.to_string()),
        _ => key_char_for(code, modifiers, caps_lock),
    };
    Some(Keystroke {
        modifiers,
        key: key.to_string(),
        key_char,
    })
}

// ── 键盘布局 ─────────────────────────────────────────────────────────────────

/// 鸿蒙键盘布局描述。
///
/// OHOS 的 InputMethod 框架不向 native 侧暴露“当前布局 id”，宿主页面通过
/// `on_keyboard_layout_change`/NAPI `lifecycle` 之外的方式无从查询，因此这里
/// 固定为 `en-US`：GPUI 只用它给快捷键展示挑符号，不参与字符生成。
pub struct OhosKeyboardLayout {
    id: String,
    name: String,
}

impl OhosKeyboardLayout {
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let name = match id.as_str() {
            "en-US" => "English (US)".to_string(),
            other => other.to_string(),
        };
        Self { id, name }
    }
}

impl PlatformKeyboardLayout for OhosKeyboardLayout {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_cover_the_whole_contiguous_range() {
        for (index, expected) in (b'a'..=b'z').enumerate() {
            let code = KEYCODE_A + index as i32;
            assert!(
                code <= KEYCODE_Z,
                "letter range must end at KEYCODE_Z (2042)"
            );
            assert_eq!(
                keycode_to_key(code),
                Some(std::str::from_utf8(&[expected]).unwrap()),
                "code {code}"
            );
        }
    }

    #[test]
    fn digits_cover_the_whole_contiguous_range() {
        for digit in 0..=9usize {
            assert_eq!(
                keycode_to_key(KEYCODE_0 + digit as i32),
                Some(&digit.to_string()[..]),
            );
        }
    }

    #[test]
    fn function_keys_cover_f1_through_f12() {
        for f in 1..=12usize {
            assert_eq!(
                keycode_to_key(KEYCODE_F1 + f as i32 - 1),
                Some(&format!("f{f}")[..])
            );
        }
    }

    #[test]
    fn navigation_keys_map_to_gpui_names() {
        let cases = [
            (KEYCODE_ENTER, "enter"),
            (KEYCODE_NUMPAD_ENTER, "enter"),
            (KEYCODE_DPAD_CENTER, "enter"),
            (KEYCODE_ESCAPE, "escape"),
            (KEYCODE_BACK, "escape"),
            (KEYCODE_DEL, "backspace"),
            (KEYCODE_FORWARD_DEL, "delete"),
            (KEYCODE_TAB, "tab"),
            (KEYCODE_DPAD_UP, "up"),
            (KEYCODE_DPAD_DOWN, "down"),
            (KEYCODE_DPAD_LEFT, "left"),
            (KEYCODE_DPAD_RIGHT, "right"),
            (KEYCODE_MOVE_HOME, "home"),
            (KEYCODE_MOVE_END, "end"),
            (KEYCODE_INSERT, "insert"),
            (KEYCODE_PAGE_UP, "pageup"),
            (KEYCODE_PAGE_DOWN, "pagedown"),
            (KEYCODE_SPACE, " "),
        ];
        for (code, expected) in cases {
            assert_eq!(keycode_to_key(code), Some(expected), "code {code}");
        }
    }

    #[test]
    fn modifier_and_system_keys_are_not_keystrokes() {
        for code in [
            KEYCODE_SHIFT_LEFT,
            KEYCODE_SHIFT_RIGHT,
            KEYCODE_CTRL_LEFT,
            KEYCODE_CTRL_RIGHT,
            KEYCODE_ALT_LEFT,
            KEYCODE_ALT_RIGHT,
            KEYCODE_META_LEFT,
            KEYCODE_META_RIGHT,
            KEYCODE_FUNCTION,
            KEYCODE_CAPS_LOCK,
            KEYCODE_NUM_LOCK,
            KEYCODE_SCROLL_LOCK,
            KEYCODE_VOLUME_UP,
            KEYCODE_VOLUME_DOWN,
            KEYCODE_HOME,
            KEYCODE_SYSRQ,
            KEYCODE_UNKNOWN,
            999_999,
        ] {
            assert_eq!(keycode_to_key(code), None, "code {code}");
            assert_eq!(
                key_event_to_keystroke(code, Modifiers::default(), false, None),
                None
            );
        }
    }

    #[test]
    fn every_modifier_code_is_tracked() {
        let codes = [
            KEYCODE_SHIFT_LEFT,
            KEYCODE_SHIFT_RIGHT,
            KEYCODE_CTRL_LEFT,
            KEYCODE_CTRL_RIGHT,
            KEYCODE_ALT_LEFT,
            KEYCODE_ALT_RIGHT,
            KEYCODE_META_LEFT,
            KEYCODE_META_RIGHT,
            KEYCODE_FUNCTION,
            KEYCODE_CAPS_LOCK,
            KEYCODE_NUM_LOCK,
            KEYCODE_SCROLL_LOCK,
        ];
        for code in codes {
            assert!(is_modifier_key(code), "code {code}");
            let mut state = ModifierState::new();
            assert!(state.apply_key(code, true));
        }
        assert!(!is_modifier_key(KEYCODE_A));
    }

    #[test]
    fn modifier_state_combines_left_and_right() {
        let mut state = ModifierState::new();
        state.apply_key(KEYCODE_SHIFT_LEFT, true);
        assert!(state.modifiers().shift);
        state.apply_key(KEYCODE_SHIFT_LEFT, false);
        assert!(!state.modifiers().shift);
        state.apply_key(KEYCODE_SHIFT_RIGHT, true);
        assert!(state.modifiers().shift);

        state.apply_key(KEYCODE_CTRL_LEFT, true);
        state.apply_key(KEYCODE_ALT_RIGHT, true);
        state.apply_key(KEYCODE_META_LEFT, true);
        let modifiers = state.modifiers();
        assert!(modifiers.control && modifiers.alt && modifiers.platform);
        assert!(!modifiers.function);
    }

    #[test]
    fn caps_lock_toggles_on_press_only() {
        let mut state = ModifierState::new();
        state.apply_key(KEYCODE_CAPS_LOCK, true);
        assert!(state.caps_lock);
        state.apply_key(KEYCODE_CAPS_LOCK, true);
        assert!(!state.caps_lock);
        state.apply_key(KEYCODE_CAPS_LOCK, false);
        assert!(!state.caps_lock, "release must not toggle");
    }

    #[test]
    fn mask_overrides_modifiers() {
        let mut state = ModifierState::new();
        state.set_from_mask(0b1011);
        let modifiers = state.modifiers();
        assert!(modifiers.shift && modifiers.control && modifiers.platform);
        assert!(!modifiers.alt);
    }

    #[test]
    fn shifted_letters_and_digits_produce_ascii() {
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            key_char_for(KEYCODE_A, shift, false).as_deref(),
            Some("A")
        );
        assert_eq!(
            key_char_for(KEYCODE_1, shift, false).as_deref(),
            Some("!")
        );
        assert_eq!(
            key_char_for(KEYCODE_SLASH, shift, false).as_deref(),
            Some("?")
        );
        assert_eq!(key_char_for(KEYCODE_ENTER, shift, false), None);
    }

    #[test]
    fn caps_lock_xors_with_shift() {
        let none = Modifiers::default();
        let b = KEYCODE_A + 1;
        assert_eq!(key_char_for(b, none, true).as_deref(), Some("B"));
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(key_char_for(b, shift, true).as_deref(), Some("b"));
    }

    #[test]
    fn keystroke_prefers_host_unicode() {
        let keystroke = key_event_to_keystroke(
            KEYCODE_A,
            Modifiers::default(),
            false,
            Some('ä'),
        )
        .unwrap();
        assert_eq!(keystroke.key, "a");
        assert_eq!(keystroke.key_char.as_deref(), Some("ä"));
    }

    #[test]
    fn keystroke_uses_key_name_for_navigation() {
        let keystroke =
            key_event_to_keystroke(KEYCODE_ENTER, Modifiers::default(), false, None).unwrap();
        assert_eq!(keystroke.key, "enter");
        assert_eq!(keystroke.key_char, None);
    }

    #[test]
    fn layout_reports_id_and_name() {
        let layout = OhosKeyboardLayout::new("en-US");
        assert_eq!(layout.id(), "en-US");
        assert_eq!(layout.name(), "English (US)");
    }
}
