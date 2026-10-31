// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Interface strings in four languages: Russian, English, Chinese, Japanese.
//!
//! The tables are static constants: they live in the executable's read-only section and are paged in
//! on demand, so unused languages take no memory. The language is chosen once at startup, from the
//! config or by the Windows interface language.

use std::sync::OnceLock;

use windows::Win32::Globalization::GetUserDefaultUILanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
    Zh,
    Ja,
}

impl Lang {
    /// Code from the config: `ru`, `en`, `zh`, `ja`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "ru" => Some(Lang::Ru),
            "en" => Some(Lang::En),
            "zh" => Some(Lang::Zh),
            "ja" => Some(Lang::Ja),
            _ => None,
        }
    }

    /// Windows interface language; an unknown one falls back to English.
    fn system() -> Self {
        // The low 10 bits of a LANGID are the primary language.
        // SAFETY: `GetUserDefaultUILanguage` has no preconditions.
        match unsafe { GetUserDefaultUILanguage() } & 0x3ff {
            0x19 => Lang::Ru,
            0x04 => Lang::Zh,
            0x11 => Lang::Ja,
            _ => Lang::En,
        }
    }
}

/// All interface strings of one language.
pub struct Strings {
    /// Locale for DirectWrite: font fallback and CJK glyph forms depend on it.
    pub locale: &'static str,
    /// Language of Open-Meteo replies (city names).
    pub api_lang: &'static str,
    pub nothing_playing: &'static str,
    pub no_player: &'static str,
    pub loading_weather: &'static str,
    pub sunrise: &'static str,
    pub sunset: &'static str,
    pub feels_like: &'static str,
    pub wind: &'static str,
    pub humidity: &'static str,
    /// Wind speed unit.
    pub ms: &'static str,
    pub menu_settings: &'static str,
    pub menu_autostart: &'static str,
    pub menu_refresh: &'static str,
    pub menu_exit: &'static str,
    /// Settings: values, rows, labels.
    pub as_windows: &'static str,
    pub theme_dark: &'static str,
    pub theme_light: &'static str,
    pub set_theme: &'static str,
    pub set_language: &'static str,
    pub set_city: &'static str,
    pub city_auto: &'static str,
    pub city_locate: &'static str,
    pub city_placeholder: &'static str,
    pub set_modules: &'static str,
    pub of: &'static str,
    pub modules_folder: &'static str,
    pub set_terminal: &'static str,
    pub off: &'static str,
    pub open_config: &'static str,
    pub logs: &'static str,
    pub log_debug: &'static str,
    pub logs_folder: &'static str,
    pub about: &'static str,
    pub version: &'static str,
    pub source: &'static str,
    pub apply: &'static str,
    pub notify_setup: &'static str,
    pub notify_admin: &'static str,
    pub notify_banners: &'static str,
    pub activate_experimental: &'static str,
    pub unsaved_title: &'static str,
    pub unsaved_text: &'static str,
    pub discard: &'static str,
    pub sec_look: &'static str,
    pub sec_weather: &'static str,
    pub sec_behaviour: &'static str,
    /// Caps/Num Lock: a short "off" on the chip, the full state in the details.
    pub lock_short_off: &'static str,
    pub lock_on: &'static str,
    pub lock_off: &'static str,
    pub keyboard: &'static str,
    /// Launcher: empty-line hint, empty results, result row type.
    pub launcher_placeholder: &'static str,
    pub launcher_empty: &'static str,
    pub launcher_nothing: &'static str,
    pub launcher_app: &'static str,
    pub launcher_calc: &'static str,
    /// Currency row label: "rate 19.10".
    pub launcher_rate: &'static str,
    /// Explorer search row when Everything is not running: "Search Windows for «…»".
    pub launcher_windows: &'static str,
    /// Row menu, in `launcher::Act` order.
    pub launcher_acts: [&'static str; 6],
    /// Long command chip: duration units, source and exit code.
    pub unit_s: &'static str,
    pub unit_min: &'static str,
    pub unit_h: &'static str,
    pub terminal: &'static str,
    pub exit_code: &'static str,
    /// Chip title suffix when a module fails: "<name> · stopped".
    pub module_stopped: &'static str,
    /// A script module's missing interpreter; `{}` is its name.
    pub module_needs: &'static str,
    /// Names of controls: tooltips on icon buttons and what a screen reader says.
    pub names: Names,
    /// Short weekday names starting from Sunday, for the pill and the forecast.
    pub dow: [&'static str; 7],
    pub months: [&'static str; 12],
    /// Weather descriptions by WMO code.
    weather: WeatherWords,
    date: fn(&Strings, dow: usize, day: u16, month: usize) -> String,
}

/// Names of controls without their own text.
pub struct Names {
    pub island: &'static str,
    pub search: &'static str,
    pub results: &'static str,
    pub notifications: &'static str,
    pub prev: &'static str,
    pub play: &'static str,
    pub pause: &'static str,
    pub next: &'static str,
    pub seek: &'static str,
    pub close: &'static str,
    pub back: &'static str,
    pub open_app: &'static str,
    /// The "+N" pile: "N more notifications".
    pub more: &'static str,
    pub copy_color: &'static str,
}

/// Weather descriptions; field order follows the code groups in [`Strings::describe`].
struct WeatherWords([&'static str; 20]);

impl Strings {
    /// Date for the pill next to the time.
    pub fn date(&self, dow: usize, day: u16, month: usize) -> String {
        (self.date)(self, dow % 7, day, month % 12)
    }

    /// Weather description by WMO code.
    pub fn describe(&self, code: u16) -> &'static str {
        let i = match code {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            45 | 48 => 4,
            51 | 53 | 55 => 5,
            56 | 57 => 6,
            61 => 7,
            63 => 8,
            65 => 9,
            66 | 67 => 10,
            71 => 11,
            73 => 12,
            75 => 13,
            77 => 14,
            80 | 81 => 15,
            82 => 16,
            85 | 86 => 17,
            95 => 18,
            96 | 99 => 19,
            _ => return "—",
        };
        self.weather.0[i]
    }
}

const RU: Strings = Strings {
    locale: "ru-ru",
    api_lang: "ru",
    nothing_playing: "Ничего не воспроизводится",
    no_player: "Плеер не активен",
    loading_weather: "Загрузка погоды…",
    sunrise: "Рассвет",
    sunset: "Закат",
    feels_like: "Ощущается как",
    wind: "Ветер",
    humidity: "Влажность",
    ms: "м/с",
    menu_settings: "Настройки",
    menu_autostart: "Автозапуск при входе",
    menu_refresh: "Обновить вид",
    menu_exit: "Выйти",
    as_windows: "Как в Windows",
    theme_dark: "Тёмная",
    theme_light: "Светлая",
    set_theme: "Тема",
    set_language: "Язык",
    set_city: "Город",
    city_auto: "Автоматически",
    city_locate: "Определять по местоположению",
    city_placeholder: "Введите город…",
    set_modules: "Модули",
    of: "из",
    modules_folder: "Папка модулей",
    set_terminal: "Уведомлять о долгих командах",
    off: "Выкл.",
    open_config: "Открыть config.ini",
    logs: "Логи",
    log_debug: "Полный лог (debug)",
    logs_folder: "Открыть папку логов",
    about: "О программе",
    version: "Версия",
    source: "Исходный код",
    apply: "Применить",
    notify_setup: "Установить",
    notify_admin: "Требуются права администратора",
    notify_banners: "Скрыть системные баннеры",
    activate_experimental: "Экспериментальный режим",
    unsaved_title: "Несохранённые изменения",
    unsaved_text: "Изменения вступят в силу после перезапуска",
    discard: "Сбросить",
    sec_look: "Внешний вид",
    sec_weather: "Погода",
    sec_behaviour: "Поведение",
    lock_short_off: "выкл",
    lock_on: "включён",
    lock_off: "выключен",
    keyboard: "Клавиатура",
    launcher_placeholder: "Поиск…",
    launcher_empty: "Введите название приложения…",
    launcher_nothing: "Ничего не найдено",
    launcher_app: "Приложение",
    launcher_calc: "Калькулятор",
    launcher_rate: "Курс валют",
    launcher_windows: "Искать в Windows",
    launcher_acts: ["Открыть", "Запустить от администратора", "Показать в проводнике", "Скопировать путь", "Убрать из недавних", "Удалить программу"],
    unit_s: "с",
    unit_min: "мин",
    unit_h: "ч",
    terminal: "Терминал",
    exit_code: "Код возврата",
    module_stopped: "остановлен",
    module_needs: "нужен {}",
    names: Names {
        island: "Полынья",
        search: "Поиск",
        results: "Результаты",
        notifications: "Уведомления",
        prev: "Предыдущий трек",
        play: "Воспроизвести",
        pause: "Пауза",
        next: "Следующий трек",
        seek: "Перемотка",
        close: "Закрыть",
        back: "Назад",
        open_app: "Открыть приложение",
        more: "Ещё уведомления",
        copy_color: "Скопировать цвет",
    },
    dow: ["вс", "пн", "вт", "ср", "чт", "пт", "сб"],
    months: ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"],
    weather: WeatherWords([
        "ясно",
        "почти ясно",
        "переменная облачность",
        "пасмурно",
        "туман",
        "морось",
        "ледяная морось",
        "небольшой дождь",
        "дождь",
        "сильный дождь",
        "ледяной дождь",
        "небольшой снег",
        "снег",
        "сильный снег",
        "снежная крупа",
        "ливень",
        "сильный ливень",
        "снегопад",
        "гроза",
        "гроза с градом",
    ]),
    date: |s, dow, day, month| format!("{}, {} {}", s.dow[dow], day, s.months[month]),
};

const EN: Strings = Strings {
    locale: "en-us",
    api_lang: "en",
    nothing_playing: "Nothing playing",
    no_player: "No active player",
    loading_weather: "Loading weather…",
    sunrise: "Sunrise",
    sunset: "Sunset",
    feels_like: "Feels like",
    wind: "Wind",
    humidity: "Humidity",
    ms: "m/s",
    menu_settings: "Settings",
    menu_autostart: "Start at sign-in",
    menu_refresh: "Refresh look",
    menu_exit: "Exit",
    as_windows: "As in Windows",
    theme_dark: "Dark",
    theme_light: "Light",
    set_theme: "Theme",
    set_language: "Language",
    set_city: "City",
    city_auto: "Automatic",
    city_locate: "Detect from location",
    city_placeholder: "Enter a city…",
    set_modules: "Modules",
    of: "of",
    modules_folder: "Modules folder",
    set_terminal: "Notify about long commands",
    off: "Off",
    open_config: "Open config.ini",
    logs: "Logs",
    log_debug: "Full log (debug)",
    logs_folder: "Open logs folder",
    about: "About",
    version: "Version",
    source: "Source code",
    apply: "Apply",
    notify_setup: "Install",
    notify_admin: "Requires administrator rights",
    notify_banners: "Hide system banners",
    activate_experimental: "Experimental mode",
    unsaved_title: "Unsaved changes",
    unsaved_text: "Changes take effect after a restart",
    discard: "Discard",
    sec_look: "Appearance",
    sec_weather: "Weather",
    sec_behaviour: "Behaviour",
    lock_short_off: "off",
    lock_on: "on",
    lock_off: "off",
    keyboard: "Keyboard",
    launcher_placeholder: "Search…",
    launcher_empty: "Enter an app name…",
    launcher_nothing: "Nothing found",
    launcher_app: "App",
    launcher_calc: "Calculator",
    launcher_rate: "Exchange rate",
    launcher_windows: "Search Windows for",
    launcher_acts: ["Open", "Run as administrator", "Show in Explorer", "Copy path", "Remove from recent", "Uninstall"],
    unit_s: "s",
    unit_min: "min",
    unit_h: "h",
    terminal: "Terminal",
    exit_code: "Exit code",
    module_stopped: "stopped",
    module_needs: "needs {}",
    names: Names {
        island: "Polynya",
        search: "Search",
        results: "Results",
        notifications: "Notifications",
        prev: "Previous track",
        play: "Play",
        pause: "Pause",
        next: "Next track",
        seek: "Seek",
        close: "Close",
        back: "Back",
        open_app: "Open the app",
        more: "More notifications",
        copy_color: "Copy the color",
    },
    dow: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    months: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
    weather: WeatherWords([
        "clear",
        "mostly clear",
        "partly cloudy",
        "overcast",
        "fog",
        "drizzle",
        "freezing drizzle",
        "light rain",
        "rain",
        "heavy rain",
        "freezing rain",
        "light snow",
        "snow",
        "heavy snow",
        "snow grains",
        "showers",
        "heavy showers",
        "snowfall",
        "thunderstorm",
        "thunderstorm with hail",
    ]),
    date: |s, dow, day, month| format!("{}, {} {}", s.dow[dow], s.months[month], day),
};

const ZH: Strings = Strings {
    locale: "zh-cn",
    api_lang: "zh",
    nothing_playing: "暂无播放",
    no_player: "没有活动的播放器",
    loading_weather: "正在加载天气…",
    sunrise: "日出",
    sunset: "日落",
    feels_like: "体感",
    wind: "风",
    humidity: "湿度",
    ms: "米/秒",
    menu_settings: "设置",
    menu_autostart: "登录时自动启动",
    menu_refresh: "刷新外观",
    menu_exit: "退出",
    as_windows: "跟随 Windows",
    theme_dark: "深色",
    theme_light: "浅色",
    set_theme: "主题",
    set_language: "语言",
    set_city: "城市",
    city_auto: "自动",
    city_locate: "根据位置确定",
    city_placeholder: "输入城市…",
    set_modules: "模块",
    of: "/",
    modules_folder: "模块文件夹",
    set_terminal: "通知长时间运行的命令",
    off: "关",
    open_config: "打开 config.ini",
    logs: "日志",
    log_debug: "完整日志 (debug)",
    logs_folder: "打开日志文件夹",
    about: "关于",
    version: "版本",
    source: "源代码",
    apply: "应用",
    notify_setup: "安装",
    notify_admin: "需要管理员权限",
    notify_banners: "隐藏系统横幅",
    activate_experimental: "实验模式",
    unsaved_title: "未保存的更改",
    unsaved_text: "更改将在重新启动后生效",
    discard: "放弃",
    sec_look: "外观",
    sec_weather: "天气",
    sec_behaviour: "行为",
    lock_short_off: "关",
    lock_on: "已开启",
    lock_off: "已关闭",
    keyboard: "键盘",
    launcher_placeholder: "搜索…",
    launcher_empty: "输入应用名称…",
    launcher_nothing: "未找到结果",
    launcher_app: "应用",
    launcher_calc: "计算器",
    launcher_rate: "汇率",
    launcher_windows: "在 Windows 中搜索",
    launcher_acts: ["打开", "以管理员身份运行", "在资源管理器中显示", "复制路径", "从最近使用中删除", "卸载"],
    unit_s: "秒",
    unit_min: "分钟",
    unit_h: "小时",
    terminal: "终端",
    exit_code: "退出代码",
    module_stopped: "已停止",
    module_needs: "需要 {}",
    names: Names {
        island: "Polynya",
        search: "搜索",
        results: "结果",
        notifications: "通知",
        prev: "上一首",
        play: "播放",
        pause: "暂停",
        next: "下一首",
        seek: "跳转",
        close: "关闭",
        back: "返回",
        open_app: "打开应用",
        more: "更多通知",
        copy_color: "复制颜色",
    },
    dow: ["周日", "周一", "周二", "周三", "周四", "周五", "周六"],
    months: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
    weather: WeatherWords([
        "晴",
        "大部晴朗",
        "局部多云",
        "阴",
        "雾",
        "毛毛雨",
        "冻毛毛雨",
        "小雨",
        "中雨",
        "大雨",
        "冻雨",
        "小雪",
        "中雪",
        "大雪",
        "米雪",
        "阵雨",
        "强阵雨",
        "阵雪",
        "雷暴",
        "雷暴伴有冰雹",
    ]),
    date: |s, dow, day, month| format!("{}月{}日 {}", month + 1, day, s.dow[dow]),
};

const JA: Strings = Strings {
    locale: "ja-jp",
    api_lang: "ja",
    nothing_playing: "再生していません",
    no_player: "アクティブなプレーヤーがありません",
    loading_weather: "天気を読み込み中…",
    sunrise: "日の出",
    sunset: "日の入り",
    feels_like: "体感",
    wind: "風",
    humidity: "湿度",
    ms: "m/s",
    menu_settings: "設定",
    menu_autostart: "サインイン時に自動起動",
    menu_refresh: "外観を更新",
    menu_exit: "終了",
    as_windows: "Windows に合わせる",
    theme_dark: "ダーク",
    theme_light: "ライト",
    set_theme: "テーマ",
    set_language: "言語",
    set_city: "都市",
    city_auto: "自動",
    city_locate: "位置情報から取得",
    city_placeholder: "都市名を入力…",
    set_modules: "モジュール",
    of: "/",
    modules_folder: "モジュール フォルダー",
    set_terminal: "長いコマンドを通知",
    off: "オフ",
    open_config: "config.ini を開く",
    logs: "ログ",
    log_debug: "詳細ログ (debug)",
    logs_folder: "ログ フォルダーを開く",
    about: "このアプリについて",
    version: "バージョン",
    source: "ソース コード",
    apply: "適用",
    notify_setup: "インストール",
    notify_admin: "管理者権限が必要です",
    notify_banners: "システムのバナーを非表示にする",
    activate_experimental: "試験モード",
    unsaved_title: "未保存の変更",
    unsaved_text: "変更は再起動後に反映されます",
    discard: "破棄",
    sec_look: "外観",
    sec_weather: "天気",
    sec_behaviour: "動作",
    lock_short_off: "オフ",
    lock_on: "オン",
    lock_off: "オフ",
    keyboard: "キーボード",
    launcher_placeholder: "検索…",
    launcher_empty: "アプリ名を入力…",
    launcher_nothing: "見つかりません",
    launcher_app: "アプリ",
    launcher_calc: "電卓",
    launcher_rate: "為替レート",
    launcher_windows: "Windows で検索",
    launcher_acts: ["開く", "管理者として実行", "エクスプローラーで表示", "パスをコピー", "最近使ったものから削除", "アンインストール"],
    unit_s: "秒",
    unit_min: "分",
    unit_h: "時間",
    terminal: "ターミナル",
    exit_code: "終了コード",
    module_stopped: "停止しました",
    module_needs: "{} が必要",
    names: Names {
        island: "Polynya",
        search: "検索",
        results: "結果",
        notifications: "通知",
        prev: "前の曲",
        play: "再生",
        pause: "一時停止",
        next: "次の曲",
        seek: "シーク",
        close: "閉じる",
        back: "戻る",
        open_app: "アプリを開く",
        more: "その他の通知",
        copy_color: "色をコピー",
    },
    dow: ["日", "月", "火", "水", "木", "金", "土"],
    months: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
    weather: WeatherWords([
        "快晴",
        "おおむね晴れ",
        "晴れ時々曇り",
        "曇り",
        "霧",
        "霧雨",
        "着氷性の霧雨",
        "小雨",
        "雨",
        "大雨",
        "着氷性の雨",
        "小雪",
        "雪",
        "大雪",
        "霧雪",
        "にわか雨",
        "激しいにわか雨",
        "にわか雪",
        "雷雨",
        "ひょうを伴う雷雨",
    ]),
    date: |s, dow, day, month| format!("{}月{}日({})", month + 1, day, s.dow[dow]),
};

static CURRENT: OnceLock<&'static Strings> = OnceLock::new();

/// Strings of a given language, regardless of the current one.
pub(crate) fn table(lang: Lang) -> &'static Strings {
    match lang {
        Lang::Ru => &RU,
        Lang::En => &EN,
        Lang::Zh => &ZH,
        Lang::Ja => &JA,
    }
}

/// Chooses the language. Called once at startup; without it the Windows language applies.
pub fn init(lang: Option<Lang>) {
    let _ = CURRENT.set(table(lang.unwrap_or_else(Lang::system)));
}

/// Strings of the current language.
pub fn t() -> &'static Strings {
    CURRENT.get_or_init(|| table(Lang::system()))
}

/// Uppercases the first letter, for the start of a list row.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_per_language() {
        assert_eq!(RU.date(0, 18, 9), "вс, 18 окт");
        assert_eq!(EN.date(0, 18, 9), "Sun, Oct 18");
        assert_eq!(ZH.date(0, 18, 9), "10月18日 周日");
        assert_eq!(JA.date(0, 18, 9), "10月18日(日)");
    }

    #[test]
    fn weather_codes_cover_groups() {
        assert_eq!(RU.describe(0), "ясно");
        assert_eq!(RU.describe(99), "гроза с градом");
        assert_eq!(EN.describe(63), "rain");
        assert_eq!(ZH.describe(95), "雷暴");
        assert_eq!(JA.describe(3), "曇り");
        assert_eq!(RU.describe(42), "—");
    }

    #[test]
    fn lang_codes() {
        assert_eq!(Lang::parse("JA"), Some(Lang::Ja));
        assert_eq!(Lang::parse("de"), None);
    }
}
