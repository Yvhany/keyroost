// crates/keyroost/src/locales.rs
//
// Dynamic i18n module for keyroost GUI translations.
// Loads language files from `language/` directory at runtime.

use std::collections::HashMap;
use std::sync::Mutex;

/// JSON 语言文件的结构
#[derive(serde::Deserialize)]
struct LanguageFile {
    language: String,
    language_name: String,
    #[serde(flatten)]
    translations: HashMap<String, String>,
}

/// 已加载的语言信息
#[derive(Clone, Debug)]
pub struct LanguageInfo {
    pub code: String,
    pub name: String,
}

/// 当前语言代码
static CURRENT_LANG: std::sync::OnceLock<Mutex<String>> = std::sync::OnceLock::new();

/// 获取系统语言代码
fn system_language() -> String {
    // Windows: 从环境变量获取
    if let Ok(lang) = std::env::var("LANG") {
        return lang;
    }
    if let Ok(lang) = std::env::var("LC_ALL") {
        return lang;
    }
    if let Ok(lang) = std::env::var("LC_MESSAGES") {
        return lang;
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(lang) = std::env::var("USERLANGUAGE") {
            return lang;
        }
        if let Ok(lang) = std::env::var("SystemDefaultUILanguage") {
            return lang;
        }
    }
    "en".to_string()
}

/// 初始化语言设置：扫描可用语言，匹配系统语言
pub fn init_language() {
    let sys_lang = system_language();
    eprintln!("[i18n] System language: {}", sys_lang);
    
    let available = available_languages();
    eprintln!("[i18n] Available languages: {:?}", available.iter().map(|l| l.code.clone()).collect::<Vec<_>>());
    
    // 始终优先使用中文，除非系统语言明确匹配其他语言
    let selected = if sys_lang == "zh-CN" || sys_lang.starts_with("zh") {
        // 系统语言是中文，用中文
        "zh-CN".to_string()
    } else if available.iter().any(|l| l.code == "zh-CN") {
        // 有中文可用，优先用中文
        "zh-CN".to_string()
    } else if let Some(lang) = available.first() {
        lang.code.clone()
    } else {
        "zh-CN".to_string()
    };
    
    eprintln!("[i18n] Selected language: {}", selected);
    set_current_language(&selected);
}

/// 设置当前语言
pub fn set_current_language(lang: &str) {
    let mutex = CURRENT_LANG.get_or_init(|| Mutex::new("en".to_string()));
    *mutex.lock().unwrap() = lang.to_string();
    eprintln!("[i18n] Language set to: {}", lang);
}

/// 获取当前语言代码
pub fn current_language() -> String {
    let mutex = CURRENT_LANG.get_or_init(|| Mutex::new("en".to_string()));
    mutex.lock().unwrap().clone()
}

/// 获取当前语言的翻译
pub fn t(key: &str) -> String {
    let lang = current_language();
    Translations::load(&lang)
        .ui_string(key)
        .unwrap_or(key)
        .to_string()
}

pub fn available_languages() -> Vec<LanguageInfo> {
    let mut langs = Vec::new();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    
    let dir = exe_dir.join("language");
    if dir.exists() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(file) = serde_json::from_str::<LanguageFile>(&content) {
                            eprintln!("[i18n] Found language file: {} -> code={}", path.display(), file.language);
                            langs.push(LanguageInfo {
                                code: file.language,
                                name: file.language_name,
                            });
                        }
                    }
                }
            }
        }
    }
    if langs.iter().all(|l| l.code != "en") {
        langs.insert(0, LanguageInfo {
            code: "en".to_string(),
            name: "English".to_string(),
        });
    }
    langs
}

/// 当前语言
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Language {
    pub code: String,
}

impl Language {
    pub fn new(code: &str) -> Self {
        Self { code: code.to_string() }
    }

    pub fn display_name(&self) -> String {
        available_languages()
            .iter()
            .find(|l| l.code == self.code)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| self.code.clone())
    }
}

impl Default for Language {
    fn default() -> Self {
        Self::new("en")
    }
}

/// 翻译存储
pub struct Translations {
    lang: String,
    ui_strings: HashMap<String, String>,
}

impl Default for Translations {
    fn default() -> Self {
        Self::load("en")
    }
}

impl Translations {
    /// 加载指定语言的翻译（从文件名加载）
    pub fn load(lang_code: &str) -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        
        let lang_dir = exe_dir.join("language");
        if !lang_dir.exists() {
            return Self { lang: lang_code.to_string(), ui_strings: HashMap::new() };
        }

        // 直接按文件名查找
        let path = lang_dir.join(format!("{}.json", lang_code));
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(file) = serde_json::from_str::<LanguageFile>(&content) {
                    return Self { lang: lang_code.to_string(), ui_strings: file.translations };
                }
            }
        }

        Self { lang: lang_code.to_string(), ui_strings: HashMap::new() }
    }

    pub fn new(lang: Language) -> Self {
        Self::load(&lang.code)
    }

    pub fn language_code(&self) -> &str {
        &self.lang
    }

    pub fn ui_string(&self, key: &str) -> Option<&str> {
        let result = self.ui_strings.get(key).map(|s| s.as_str());
        if result.is_none() {
            eprintln!("[i18n] Missing key: '{}' (language: {})", key, self.lang);
        }
        result
    }

    pub fn help_title(&self, key: &str) -> Option<&str> {
        let json_key = format!("help_title_{}", key);
        self.ui_strings.get(&json_key).map(|s| s.as_str())
    }

    pub fn help_body(&self, key: &str) -> Option<&str> {
        let json_key = format!("help_body_{}", key);
        self.ui_strings.get(&json_key).map(|s| s.as_str())
    }
}
