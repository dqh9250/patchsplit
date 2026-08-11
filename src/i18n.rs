use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

const DOMAIN: &str = "patchsplit";

const ZH_CN: &str = include_str!("../po/zh_CN.po");

static CATALOG: OnceLock<Catalog> = OnceLock::new();

pub fn init() {
    let _ = CATALOG.set(Catalog::load());
}

pub fn tr(msgid: &'static str) -> String {
    catalog().translate(msgid).to_string()
}

pub fn tr_args(msgid: &'static str, args: &[(&str, String)]) -> String {
    interpolate(catalog().translate(msgid), args)
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(Catalog::load)
}

#[derive(Debug, Default)]
struct Catalog {
    messages: HashMap<String, String>,
}

impl Catalog {
    fn load() -> Self {
        for locale in requested_locales() {
            for path in catalog_paths(&locale) {
                let Ok(contents) = fs::read_to_string(&path) else {
                    continue;
                };

                return Self {
                    messages: parse_po(&contents),
                };
            }

            if let Some(contents) = built_in_catalog(&locale) {
                return Self {
                    messages: parse_po(contents),
                };
            }
        }
        Self::default()
    }

    fn translate<'a>(&'a self, msgid: &'a str) -> &'a str {
        self.messages
            .get(msgid)
            .map(String::as_str)
            .filter(|msgstr| !msgstr.is_empty())
            .unwrap_or(msgid)
    }
}

fn built_in_catalog(locale: &str) -> Option<&'static str> {
    match locale {
        "zh_CN" | "zh" => Some(ZH_CN),
        _ => None,
    }
}

fn requested_locales() -> Vec<String> {
    requested_locales_with(locale_env)
}

fn requested_locales_with<F>(get_locale: F) -> Vec<String>
where
    F: Fn(&str) -> Option<String>,
{
    let mut locales = Vec::new();

    for name in ["PATCHSPLIT_LANGUAGE", "LANGUAGE"] {
        if let Some(value) = get_locale(name) {
            if is_default_locale(&value) {
                return locales;
            }

            for locale in value.split(':') {
                push_locale_variants(locale, &mut locales);
            }
            if !locales.is_empty() {
                return locales;
            }
        }
    }

    for name in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(value) = get_locale(name) {
            push_locale_variants(&value, &mut locales);
            break;
        }
    }

    locales
}

fn locale_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn push_locale_variants(value: &str, locales: &mut Vec<String>) {
    let locale = locale_name(value);

    if locale.is_empty() || is_default_locale(locale) {
        return;
    }

    let normalized = normalize_locale(locale);
    push_unique(locales, normalized.clone());

    if let Some((language, _region)) = normalized.split_once('_') {
        if !language.is_empty() {
            push_unique(locales, language.to_string());
        }
    }
}

fn is_default_locale(value: &str) -> bool {
    matches!(locale_name(value), "C" | "POSIX")
}

fn locale_name(value: &str) -> &str {
    value
        .split('.')
        .next()
        .unwrap_or(value)
        .split('@')
        .next()
        .unwrap_or(value)
}

fn normalize_locale(value: &str) -> String {
    let normalized = value.replace('-', "_");
    let Some((language, rest)) = normalized.split_once('_') else {
        return normalized.to_ascii_lowercase();
    };

    let mut locale = language.to_ascii_lowercase();
    locale.push('_');
    locale.push_str(&rest.to_ascii_uppercase());
    locale
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn catalog_paths(locale: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(path) = env::var("PATCHSPLIT_LOCALEDIR") {
        roots.push(PathBuf::from(path));
    }

    if let Ok(executable) = env::current_exe() {
        if let Some(executable_dir) = executable.parent() {
            roots.push(executable_dir.join("locale"));
            roots.push(executable_dir.join("po"));
            roots.push(executable_dir.join("..").join("share").join("locale"));
        }
    }

    let mut paths = Vec::new();
    for root in roots {
        paths.push(
            root.join(locale)
                .join("LC_MESSAGES")
                .join(format!("{DOMAIN}.po")),
        );
        paths.push(root.join(format!("{locale}.po")));
        paths.push(root.join(locale).join(format!("{DOMAIN}.po")));
    }

    paths
}

fn interpolate(message: &str, args: &[(&str, String)]) -> String {
    let values: HashMap<&str, &str> = args
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let mut output = String::with_capacity(message.len());
    let mut rest = message;

    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('}') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let name = &after_start[..end];
        if let Some(value) = values.get(name) {
            output.push_str(value);
        } else {
            output.push_str(&rest[start..start + end + 2]);
        }
        rest = &after_start[end + 1..];
    }

    output.push_str(rest);
    output
}

#[derive(Debug, Default)]
struct PoEntry {
    context: Option<String>,
    msgid: Option<String>,
    msgstr: Option<String>,
    fuzzy: bool,
}

#[derive(Debug, Clone, Copy)]
enum PoField {
    Context,
    Id,
    String,
    Other,
}

fn parse_po(input: &str) -> HashMap<String, String> {
    let mut messages = HashMap::new();
    let mut entry = PoEntry::default();
    let mut field = None::<PoField>;

    for raw_line in input.lines() {
        let line = raw_line.trim_start();

        if line.is_empty() {
            flush_entry(&mut messages, &mut entry);
            field = None;
            continue;
        }

        if let Some(flags) = line.strip_prefix("#,") {
            if flags.split(',').any(|flag| flag.trim() == "fuzzy") {
                entry.fuzzy = true;
            }
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        if let Some(value) = line.strip_prefix("msgctxt ") {
            entry.context = parse_po_string(value);
            field = Some(PoField::Context);
            continue;
        }

        if let Some(value) = line.strip_prefix("msgid ") {
            entry.msgid = parse_po_string(value);
            field = Some(PoField::Id);
            continue;
        }

        if line.starts_with("msgid_plural ") || line.starts_with("msgstr[") {
            field = Some(PoField::Other);
            continue;
        }

        if let Some(value) = line.strip_prefix("msgstr ") {
            entry.msgstr = parse_po_string(value);
            field = Some(PoField::String);
            continue;
        }

        if line.starts_with('"') {
            append_po_string(&mut entry, field, line);
        }
    }

    flush_entry(&mut messages, &mut entry);
    messages
}

fn flush_entry(messages: &mut HashMap<String, String>, entry: &mut PoEntry) {
    if entry.fuzzy || entry.context.is_some() {
        *entry = PoEntry::default();
        return;
    }

    if let (Some(msgid), Some(msgstr)) = (entry.msgid.take(), entry.msgstr.take()) {
        if !msgid.is_empty() && !msgstr.is_empty() {
            messages.insert(msgid, msgstr);
        }
    }

    *entry = PoEntry::default();
}

fn append_po_string(entry: &mut PoEntry, field: Option<PoField>, line: &str) {
    let Some(value) = parse_po_string(line) else {
        return;
    };

    match field {
        Some(PoField::Context) => {
            if let Some(context) = entry.context.as_mut() {
                context.push_str(&value);
            }
        }
        Some(PoField::Id) => {
            if let Some(msgid) = entry.msgid.as_mut() {
                msgid.push_str(&value);
            }
        }
        Some(PoField::String) => {
            if let Some(msgstr) = entry.msgstr.as_mut() {
                msgstr.push_str(&value);
            }
        }
        Some(PoField::Other) | None => {}
    }
}

fn parse_po_string(value: &str) -> Option<String> {
    let mut chars = value.trim_start().chars();
    if chars.next()? != '"' {
        return None;
    }

    let mut output = String::new();
    let mut escaped = false;

    for character in chars {
        if escaped {
            match character {
                'n' => output.push('\n'),
                'r' => output.push('\r'),
                't' => output.push('\t'),
                '"' => output.push('"'),
                '\\' => output.push('\\'),
                other => output.push(other),
            }
            escaped = false;
            continue;
        }

        match character {
            '\\' => escaped = true,
            '"' => return Some(output),
            other => output.push(other),
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        built_in_catalog, interpolate, parse_po, push_locale_variants, requested_locales_with,
    };

    #[test]
    fn parses_multiline_po_entries() {
        let catalog = parse_po(
            r#"
msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"

msgid "downloaded {url}"
msgstr "downloaded translated {url}"

msgid ""
"Usage:\n"
"  patchsplit\n"
msgstr ""
"Usage translated:\n"
"  patchsplit\n"

#, fuzzy
msgid "fuzzy"
msgstr "ignored"

msgctxt "button"
msgid "downloaded {url}"
msgstr "ignored with context"
"#,
        );

        assert_eq!(
            catalog.get("downloaded {url}").map(String::as_str),
            Some("downloaded translated {url}")
        );
        assert_eq!(
            catalog.get("Usage:\n  patchsplit\n").map(String::as_str),
            Some("Usage translated:\n  patchsplit\n")
        );
        assert!(!catalog.contains_key("fuzzy"));
    }

    #[test]
    fn interpolates_named_values() {
        let message = interpolate(
            "wrote {count} patch file(s) to {directory}",
            &[
                ("count", "2".to_string()),
                ("directory", "patches".to_string()),
            ],
        );

        assert_eq!(message, "wrote 2 patch file(s) to patches");
    }

    #[test]
    fn does_not_reprocess_interpolated_values() {
        let message = interpolate(
            "{path}: {source}",
            &[
                ("path", "/tmp/{source}".to_string()),
                ("source", "input".to_string()),
            ],
        );

        assert_eq!(message, "/tmp/{source}: input");
    }

    #[test]
    fn c_locale_stops_before_lower_priority_locales() {
        let locales = requested_locales_with(|name| match name {
            "PATCHSPLIT_LANGUAGE" => Some("C.UTF-8".to_string()),
            "LANG" => Some("zh_CN.UTF-8".to_string()),
            _ => None,
        });

        assert!(locales.is_empty());
    }

    #[test]
    fn invalid_language_value_falls_back_to_lang() {
        let locales = requested_locales_with(|name| match name {
            "LANGUAGE" => Some(":".to_string()),
            "LANG" => Some("zh_CN.UTF-8".to_string()),
            _ => None,
        });

        assert_eq!(locales, vec!["zh_CN".to_string(), "zh".to_string()]);
    }

    #[test]
    fn expands_locale_variants() {
        let mut locales = Vec::new();
        push_locale_variants("zh-cn.UTF-8", &mut locales);
        assert_eq!(locales, vec!["zh_CN".to_string(), "zh".to_string()]);
    }

    #[test]
    fn generic_chinese_locale_uses_bundled_catalog() {
        assert!(built_in_catalog("zh").is_some());
    }
}
