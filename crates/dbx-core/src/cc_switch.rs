use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::{AiApiStyle, AiAuthMethod, AiConfig, AiConfigItem, AiModelListItem, AiProvider};
use crate::plugins::{PluginManifest, SUPPORTED_PLUGIN_PROTOCOL_VERSION};

const DEFAULT_ANTHROPIC_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-20250514";
const DEFAULT_GEMINI_ENDPOINT: &str = "https://generativelanguage.googleapis.com";
const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
pub const CC_SWITCH_PLUGIN_ID: &str = "cc-switch";
pub const CC_SWITCH_PLUGIN_CAPABILITY: &str = "ai-config-import";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const CC_SWITCH_PLUGIN_ARTIFACT: &str = "macos-aarch64";
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const CC_SWITCH_PLUGIN_ARTIFACT: &str = "macos-x86_64";
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const CC_SWITCH_PLUGIN_ARTIFACT: &str = "windows-x86_64";
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
const CC_SWITCH_PLUGIN_ARTIFACT: &str = "windows-aarch64";
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const CC_SWITCH_PLUGIN_ARTIFACT: &str = "linux-x86_64";
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const CC_SWITCH_PLUGIN_ARTIFACT: &str = "linux-aarch64";

const CC_SWITCH_PLUGIN_DOWNLOAD_PREFIX: &str =
    "https://github.com/t8y2/dbx/releases/latest/download/dbx-cc-switch-plugin-";
const CC_SWITCH_PLUGIN_R2_PREFIX: &str = "releases/latest/dbx-cc-switch-plugin-";

fn cc_switch_plugin_download_url() -> String {
    format!("{CC_SWITCH_PLUGIN_DOWNLOAD_PREFIX}{CC_SWITCH_PLUGIN_ARTIFACT}-latest.zip")
}

fn cc_switch_plugin_r2_path() -> String {
    format!("{CC_SWITCH_PLUGIN_R2_PREFIX}{CC_SWITCH_PLUGIN_ARTIFACT}-latest.zip")
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchImportSkipped {
    pub app_type: String,
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchImportResult {
    pub configs: Vec<AiConfigItem>,
    pub skipped: Vec<CcSwitchImportSkipped>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CcSwitchPluginStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub protocol_version: Option<u32>,
    pub compatible: bool,
    pub path: String,
}

pub fn cc_switch_plugin_status(plugins_root: &Path) -> Result<CcSwitchPluginStatus, String> {
    let plugin_dir = plugins_root.join(CC_SWITCH_PLUGIN_ID);
    let manifest_path = plugin_dir.join("manifest.json");
    let manifest = match std::fs::read_to_string(&manifest_path) {
        Ok(raw) => Some(serde_json::from_str::<PluginManifest>(&raw).map_err(|error| error.to_string())?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    Ok(CcSwitchPluginStatus {
        installed: manifest.is_some(),
        version: manifest.as_ref().and_then(|item| (!item.version.is_empty()).then_some(item.version.clone())),
        protocol_version: manifest.as_ref().map(|item| item.protocol_version),
        compatible: manifest
            .as_ref()
            .is_none_or(|item| item.protocol_version == SUPPORTED_PLUGIN_PROTOCOL_VERSION),
        path: plugin_dir.to_string_lossy().to_string(),
    })
}

pub async fn install_cc_switch_plugin(plugins_root: &Path) -> Result<CcSwitchPluginStatus, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|error| error.to_string())?;
    let response = crate::race_download(
        &client,
        &cc_switch_plugin_download_url(),
        &cc_switch_plugin_r2_path(),
        "dbx-cc-switch-plugin-installer",
    )
    .await
    .map_err(|error| format!("ccSwitchPluginDownloadFailed:{error}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("ccSwitchPluginDownloadFailed:{error}"))?
        .to_vec();
    let plugins_root = plugins_root.to_path_buf();
    let install_root = plugins_root.clone();
    tokio::task::spawn_blocking(move || install_cc_switch_plugin_zip(&bytes, &install_root))
        .await
        .map_err(|error| error.to_string())??;
    cc_switch_plugin_status(&plugins_root)
}

pub async fn install_cc_switch_plugin_from_file(
    plugins_root: &Path,
    file_path: &str,
) -> Result<CcSwitchPluginStatus, String> {
    let plugins_root = plugins_root.to_path_buf();
    let file_path = file_path.to_string();
    let install_root = plugins_root.clone();
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&file_path).map_err(|error| format!("ccSwitchPluginReadFailed:{error}"))?;
        install_cc_switch_plugin_zip(&bytes, &install_root)
    })
    .await
    .map_err(|error| error.to_string())??;
    cc_switch_plugin_status(&plugins_root)
}

pub fn uninstall_cc_switch_plugin(plugins_root: &Path) -> Result<CcSwitchPluginStatus, String> {
    let plugin_dir = plugins_root.join(CC_SWITCH_PLUGIN_ID);
    if plugin_dir.exists() {
        std::fs::remove_dir_all(&plugin_dir).map_err(|error| error.to_string())?;
    }
    cc_switch_plugin_status(plugins_root)
}

fn install_cc_switch_plugin_zip(bytes: &[u8], plugins_root: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let plugin_dir = plugins_root.join(CC_SWITCH_PLUGIN_ID);
    let staging_dir = plugins_root.join(format!(".{CC_SWITCH_PLUGIN_ID}-install-{}", uuid::Uuid::new_v4()));
    if staging_dir.exists() {
        std::fs::remove_dir_all(&staging_dir).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&staging_dir).map_err(|error| error.to_string())?;

    let result = (|| {
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
            if file.is_dir() {
                continue;
            }
            let Some(enclosed) = file.enclosed_name().map(PathBuf::from) else {
                continue;
            };
            let relative = strip_zip_root(&enclosed);
            if relative.as_os_str().is_empty() {
                continue;
            }
            let output = staging_dir.join(relative);
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut target = std::fs::File::create(&output).map_err(|error| error.to_string())?;
            std::io::copy(&mut file, &mut target).map_err(|error| error.to_string())?;
        }

        let manifest_path = staging_dir.join("manifest.json");
        let raw = std::fs::read_to_string(&manifest_path)
            .map_err(|error| format!("ccSwitchPluginManifestReadFailed:{error}"))?;
        let manifest: PluginManifest = serde_json::from_str(&raw)
            .map_err(|error| format!("ccSwitchPluginManifestInvalid:{error}"))?;
        if manifest.id != CC_SWITCH_PLUGIN_ID {
            return Err(format!("ccSwitchPluginUnexpectedId:{}", manifest.id));
        }
        if manifest.protocol_version != SUPPORTED_PLUGIN_PROTOCOL_VERSION {
            return Err(format!(
                "ccSwitchPluginProtocolUnsupported:{}",
                manifest.protocol_version
            ));
        }
        if !manifest
            .capabilities
            .iter()
            .any(|capability| capability.id == CC_SWITCH_PLUGIN_CAPABILITY)
        {
            return Err("ccSwitchPluginCapabilityMissing".to_string());
        }
        let executable = manifest
            .executable
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "ccSwitchPluginExecutableMissing".to_string())?;
        if !plugin_executable_exists(&staging_dir, executable) {
            return Err("ccSwitchPluginExecutableMissing".to_string());
        }
        if plugin_dir.exists() {
            std::fs::remove_dir_all(&plugin_dir).map_err(|error| error.to_string())?;
        }
        std::fs::rename(&staging_dir, &plugin_dir).map_err(|error| error.to_string())?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let executable_path = plugin_dir.join(executable);
            let executable_path = if executable_path.is_file() {
                executable_path
            } else {
                resolve_platform_executable(&plugin_dir, executable)
            };
            let mut permissions = std::fs::metadata(&executable_path)
                .map_err(|error| error.to_string())?
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(executable_path, permissions).map_err(|error| error.to_string())?;
        }
        Ok(())
    })();
    if result.is_err() && staging_dir.exists() {
        let _ = std::fs::remove_dir_all(&staging_dir);
    }
    result
}

fn plugin_executable_exists(plugin_dir: &Path, executable: &str) -> bool {
    let path = plugin_dir.join(executable);
    path.is_file() || resolve_platform_executable(plugin_dir, executable).is_file()
}

fn resolve_platform_executable(plugin_dir: &Path, executable: &str) -> PathBuf {
    let path = plugin_dir.join(executable);

    #[cfg(windows)]
    {
        if path.extension().is_none() {
            for extension in ["exe", "bat"] {
                let candidate = path.with_extension(extension);
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }

    path
}

fn strip_zip_root(path: &Path) -> PathBuf {
    let mut components = path.components();
    components.next();
    components.as_path().to_path_buf()
}

/// Read AI providers from a CC-SWITCH database without ever opening it for writing.
pub fn load_ai_configs_from_path(path: &Path) -> Result<CcSwitchImportResult, String> {
    if !path.is_file() {
        return Err("ccSwitchNotInstalled".to_string());
    }

    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("ccSwitchOpenFailed:{error}"))?;
    load_ai_configs_from_connection(&connection)
}

pub fn load_ai_configs_from_connection(connection: &Connection) -> Result<CcSwitchImportResult, String> {
    let providers_table_exists: bool = connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'providers')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("ccSwitchReadFailed:{error}"))?;
    if !providers_table_exists {
        return Err("ccSwitchInvalidDatabase".to_string());
    }

    let mut statement = connection
        .prepare(
            "SELECT app_type, name, settings_config
             FROM providers
             WHERE app_type IN ('codex', 'claude', 'gemini')
             ORDER BY is_current DESC, sort_index ASC, name COLLATE NOCASE ASC",
        )
        .map_err(|error| format!("ccSwitchReadFailed:{error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| format!("ccSwitchReadFailed:{error}"))?;

    let mut result = CcSwitchImportResult {
        configs: Vec::new(),
        skipped: Vec::new(),
    };
    for row in rows {
        let (app_type, name, settings_config) =
            row.map_err(|error| format!("ccSwitchReadFailed:{error}"))?;
        match import_provider(&app_type, &settings_config) {
            Ok(Some(config)) => result.configs.push(AiConfigItem {
                id: AiConfigItem::new_id(),
                name,
                is_default: false,
                config,
            }),
            Ok(None) => result.skipped.push(CcSwitchImportSkipped {
                app_type,
                name,
                reason: "ccSwitchProviderNotConfigured".to_string(),
            }),
            Err(reason) => result.skipped.push(CcSwitchImportSkipped {
                app_type,
                name,
                reason,
            }),
        }
    }

    Ok(result)
}

fn import_provider(app_type: &str, raw_settings: &str) -> Result<Option<AiConfig>, String> {
    let settings: Value = serde_json::from_str(raw_settings)
        .map_err(|_| "ccSwitchInvalidProviderConfig".to_string())?;
    match app_type {
        "codex" => import_codex(&settings),
        "claude" => import_claude(&settings),
        "gemini" => import_gemini(&settings),
        _ => Ok(None),
    }
}

fn import_codex(settings: &Value) -> Result<Option<AiConfig>, String> {
    let config_text = settings.get("config").and_then(Value::as_str).unwrap_or_default();
    let document = parse_codex_toml(config_text)?;
    let model_provider = document
        .string("model_provider")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "custom".to_string());
    let provider_section = format!("model_providers.{}", normalize_toml_key(&model_provider));

    let endpoint = document
        .string(&format!("{provider_section}.base_url"))
        .or_else(|| document.string("base_url"))
        .or_else(|| {
            find_string(
                settings,
                &["base_url", "baseUrl", "OPENAI_BASE_URL"],
                &["auth", "env", "config"],
            )
        });
    let model = document
        .string("model")
        .or_else(|| find_string(settings, &["model", "model_name", "modelName"], &["config", "env"]));
    let api_key = find_named_string(
        settings,
        &["OPENAI_API_KEY", "api_key", "apiKey", "API_KEY"],
        &["auth", "env", "config"],
    );
    let requires_openai_auth = document.bool(&format!("{provider_section}.requires_openai_auth")).unwrap_or(false);

    let Some(endpoint) = endpoint.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let Some(model) = model.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    if requires_openai_auth && api_key.as_ref().is_none_or(|(_, value)| value.trim().is_empty()) {
        return Ok(None);
    }

    let api_style = document
        .string(&format!("{provider_section}.wire_api"))
        .or_else(|| document.string("wire_api"))
        .is_some_and(|value| value.eq_ignore_ascii_case("responses"))
        .then_some(AiApiStyle::Responses)
        .unwrap_or_default();
    Ok(Some(new_config(
        AiProvider::OpenaiCompatible,
        api_key.map(|(_, value)| value).unwrap_or_default(),
        endpoint,
        model,
        api_style,
        AiAuthMethod::Bearer,
    )))
}

fn import_claude(settings: &Value) -> Result<Option<AiConfig>, String> {
    let api_key = find_named_string(
        settings,
        &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY", "api_key", "apiKey", "API_KEY"],
        &["env", "auth", "config"],
    );
    let Some((key_name, api_key)) = api_key.filter(|(_, value)| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let endpoint = find_string(
        settings,
        &["ANTHROPIC_BASE_URL", "base_url", "baseUrl"],
        &["env", "auth", "config"],
    )
    .unwrap_or_else(|| DEFAULT_ANTHROPIC_ENDPOINT.to_string());
    let model = find_string(
        settings,
        &["ANTHROPIC_MODEL", "model", "model_name", "modelName"],
        &["env", "config"],
    )
    .unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_string());
    let provider = if is_default_anthropic_endpoint(&endpoint) {
        AiProvider::Claude
    } else {
        AiProvider::AnthropicCompatible
    };
    let auth_method = if key_name.eq_ignore_ascii_case("ANTHROPIC_AUTH_TOKEN") {
        AiAuthMethod::Bearer
    } else {
        AiAuthMethod::ApiKey
    };
    Ok(Some(new_config(
        provider,
        api_key,
        endpoint,
        model,
        AiApiStyle::AnthropicMessages,
        auth_method,
    )))
}

fn import_gemini(settings: &Value) -> Result<Option<AiConfig>, String> {
    let Some((_, api_key)) = find_named_string(
        settings,
        &["GEMINI_API_KEY", "GOOGLE_API_KEY", "api_key", "apiKey", "API_KEY"],
        &["env", "auth", "config"],
    )
    .filter(|(_, value)| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let endpoint = find_string(
        settings,
        &["GEMINI_BASE_URL", "GOOGLE_GEMINI_BASE_URL", "base_url", "baseUrl"],
        &["env", "auth", "config"],
    )
    .unwrap_or_else(|| DEFAULT_GEMINI_ENDPOINT.to_string());
    let model = find_string(
        settings,
        &["model", "model_name", "modelName", "GEMINI_MODEL"],
        &["config", "env"],
    )
    .unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_string());
    Ok(Some(new_config(
        AiProvider::Gemini,
        api_key,
        endpoint,
        model,
        AiApiStyle::Completions,
        AiAuthMethod::ApiKey,
    )))
}

fn new_config(
    provider: AiProvider,
    api_key: String,
    endpoint: String,
    model: String,
    api_style: AiApiStyle,
    auth_method: AiAuthMethod,
) -> AiConfig {
    AiConfig {
        provider,
        api_key,
        auth_method,
        endpoint: endpoint.trim().trim_end_matches('/').to_string(),
        models: vec![AiModelListItem {
            name: model.trim().to_string(),
            label: None,
            supported_effort_levels: Vec::new(),
        }],
        model: model.trim().to_string(),
        api_style,
        custom_headers: HashMap::new(),
        proxy_enabled: false,
        proxy_url: String::new(),
        skip_tls_verify: false,
        enable_thinking: true,
        reasoning_level: Default::default(),
        runtime_effort: None,
        max_output_tokens: None,
        context_window: None,
        max_retries: None,
        codex_cli_path: None,
        codex_cli_env: HashMap::new(),
        claude_code_cli_path: None,
        claude_code_cli_env: HashMap::new(),
        pi_agent_cli_path: None,
        pi_agent_cli_env: HashMap::new(),
        opencode_cli_path: None,
        opencode_cli_env: HashMap::new(),
        cursor_cli_path: None,
        cursor_cli_env: HashMap::new(),
        grok_cli_path: None,
        grok_cli_env: HashMap::new(),
        codebuddy_cli_path: None,
        codebuddy_cli_env: HashMap::new(),
        qoder_cli_path: None,
        qoder_cli_env: HashMap::new(),
    }
}

fn is_default_anthropic_endpoint(endpoint: &str) -> bool {
    endpoint
        .trim()
        .trim_end_matches('/')
        .eq_ignore_ascii_case(DEFAULT_ANTHROPIC_ENDPOINT.trim_end_matches('/'))
        || endpoint.trim().trim_end_matches('/').eq_ignore_ascii_case("https://api.anthropic.com")
}

fn find_string(settings: &Value, keys: &[&str], containers: &[&str]) -> Option<String> {
    find_named_string(settings, keys, containers).map(|(_, value)| value)
}

fn find_named_string(
    settings: &Value,
    keys: &[&str],
    containers: &[&str],
) -> Option<(String, String)> {
    for container in containers {
        if let Some(object) = settings.get(*container).and_then(Value::as_object) {
            if let Some(found) = object_string(object, keys) {
                return Some(found);
            }
        }
    }
    settings.as_object().and_then(|object| object_string(object, keys))
}

fn object_string(
    object: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<(String, String)> {
    keys.iter().find_map(|key| {
        object.get(*key).and_then(Value::as_str).map(|value| ((*key).to_string(), value.to_string()))
    })
}

#[derive(Default)]
struct TomlDocument {
    values: HashMap<String, TomlScalar>,
}

#[derive(Clone)]
enum TomlScalar {
    String(String),
    Bool(bool),
}

impl TomlDocument {
    fn string(&self, key: &str) -> Option<String> {
        match self.values.get(&normalize_toml_key(key)) {
            Some(TomlScalar::String(value)) => Some(value.clone()),
            _ => None,
        }
    }

    fn bool(&self, key: &str) -> Option<bool> {
        match self.values.get(&normalize_toml_key(key)) {
            Some(TomlScalar::Bool(value)) => Some(*value),
            _ => None,
        }
    }
}

fn parse_codex_toml(input: &str) -> Result<TomlDocument, String> {
    let mut document = TomlDocument::default();
    let mut section = String::new();
    for raw_line in input.lines() {
        let uncommented_line = strip_toml_comment(raw_line);
        let line = uncommented_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            let is_array = line.starts_with("[[");
            let (opening, closing) = if is_array { ("[[", "]]") } else { ("[", "]") };
            let Some(header) = line.strip_prefix(opening).and_then(|value| value.strip_suffix(closing)) else {
                return Err("ccSwitchInvalidCodexConfig".to_string());
            };
            section = normalize_toml_key(header);
            continue;
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = normalize_toml_key(raw_key);
        if key.is_empty() {
            return Err("ccSwitchInvalidCodexConfig".to_string());
        }
        let full_key = if section.is_empty() { key } else { format!("{section}.{key}") };
        if let Some(value) = parse_toml_scalar(raw_value.trim()) {
            document.values.insert(full_key, value);
        }
    }
    Ok(document)
}

fn parse_toml_scalar(value: &str) -> Option<TomlScalar> {
    if value.eq_ignore_ascii_case("true") {
        return Some(TomlScalar::Bool(true));
    }
    if value.eq_ignore_ascii_case("false") {
        return Some(TomlScalar::Bool(false));
    }
    parse_toml_string(value).map(TomlScalar::String)
}

fn parse_toml_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.starts_with('"') {
        return serde_json::from_str(value).ok();
    }
    value.strip_prefix('\'').and_then(|value| value.strip_suffix('\'')).map(str::to_string)
}

fn strip_toml_comment(line: &str) -> String {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if let Some(quote_character) = quote {
            if quote_character == '"' {
                if character == '\\' && !escaped {
                    escaped = true;
                    continue;
                }
                if character == '"' && !escaped {
                    quote = None;
                }
                escaped = false;
            } else if character == '\'' {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '#' => return line[..index].to_string(),
            _ => {}
        }
    }
    line.to_string()
}

fn normalize_toml_key(key: &str) -> String {
    key.split('.').map(str::trim).filter(|part| !part.is_empty()).map(unquote_toml_key).collect::<Vec<_>>().join(".")
}

fn unquote_toml_key(key: &str) -> String {
    key.strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| key.strip_prefix('\'').and_then(|value| value.strip_suffix('\'')))
        .unwrap_or(key)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        cc_switch_plugin_status, install_cc_switch_plugin_zip, load_ai_configs_from_connection, parse_codex_toml,
        uninstall_cc_switch_plugin, CC_SWITCH_PLUGIN_CAPABILITY, CC_SWITCH_PLUGIN_ID,
    };
    use rusqlite::Connection;
    use std::io::Write;

    #[test]
    fn imports_current_codex_gateway_and_skips_empty_official_profile() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE providers (
                    id TEXT NOT NULL,
                    app_type TEXT NOT NULL,
                    name TEXT NOT NULL,
                    settings_config TEXT NOT NULL,
                    sort_index INTEGER,
                    is_current BOOLEAN NOT NULL DEFAULT 0
                );
                INSERT INTO providers (id, app_type, name, settings_config, sort_index, is_current)
                VALUES
                  ('gateway', 'codex', 'Gateway', '{\"auth\":{\"OPENAI_API_KEY\":\"secret\"},\"config\":\"model_provider = \\\"custom\\\"\\nmodel = \\\"gpt-5.6-luna\\\"\\n[model_providers.custom]\\nname = \\\"Gateway\\\"\\nbase_url = \\\"https://gateway.example\\\"\\nwire_api = \\\"responses\\\"\\nrequires_openai_auth = true\\n\"}', 0, 1),
                  ('official', 'codex', 'OpenAI Official', '{\"auth\":{},\"config\":\"\"}', 1, 0);",
            )
            .unwrap();

        let result = load_ai_configs_from_connection(&connection).unwrap();
        assert_eq!(result.configs.len(), 1);
        assert_eq!(result.configs[0].config.model, "gpt-5.6-luna");
        assert_eq!(result.configs[0].config.endpoint, "https://gateway.example");
        assert_eq!(result.configs[0].config.api_key, "secret");
        assert_eq!(result.configs[0].config.api_style, crate::ai::AiApiStyle::Responses);
        assert_eq!(result.skipped[0].reason, "ccSwitchProviderNotConfigured");
    }

    #[test]
    fn parses_comments_quotes_and_provider_sections() {
        let document = parse_codex_toml(
            r#"
            model_provider = "custom" # active provider
            model = 'gpt-test'
            [model_providers.custom]
            base_url = "https://example.test/api#v1"
            requires_openai_auth = false
            "#,
        )
        .unwrap();
        assert_eq!(document.string("model_provider").as_deref(), Some("custom"));
        assert_eq!(document.string("model").as_deref(), Some("gpt-test"));
        assert_eq!(document.string("model_providers.custom.base_url").as_deref(), Some("https://example.test/api#v1"));
        assert_eq!(document.bool("model_providers.custom.requires_openai_auth"), Some(false));
    }

    #[test]
    fn maps_claude_auth_token_to_bearer_and_custom_provider() {
        let settings = serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "token",
                "ANTHROPIC_BASE_URL": "https://gateway.example"
            }
        });
        let config = super::import_claude(&settings).unwrap().unwrap();
        assert_eq!(config.provider.as_str(), "anthropic-compatible");
        assert_eq!(config.auth_method, crate::ai::AiAuthMethod::Bearer);
        assert_eq!(config.api_style, crate::ai::AiApiStyle::AnthropicMessages);
        assert_eq!(config.endpoint, "https://gateway.example");
    }

    #[test]
    fn maps_gemini_api_key_and_default_endpoint() {
        let settings = serde_json::json!({ "env": { "GEMINI_API_KEY": "key" } });
        let config = super::import_gemini(&settings).unwrap().unwrap();
        assert_eq!(config.provider.as_str(), "gemini");
        assert_eq!(config.endpoint, "https://generativelanguage.googleapis.com");
        assert_eq!(config.model, "gemini-2.5-flash");
    }

    #[test]
    fn installs_reports_and_uninstalls_cc_switch_plugin_zip() {
        let root = tempfile::tempdir().unwrap();
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut bytes);
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            archive.start_file(format!("{CC_SWITCH_PLUGIN_ID}/manifest.json"), options).unwrap();
            archive
                .write_all(
                    serde_json::to_string(&serde_json::json!({
                        "id": CC_SWITCH_PLUGIN_ID,
                        "name": "CC-SWITCH",
                        "version": "test",
                        "protocol_version": 1,
                        "executable": "bin/plugin",
                        "capabilities": [{"id": CC_SWITCH_PLUGIN_CAPABILITY, "label": "Import", "kind": "ai-config-import"}]
                    }))
                    .unwrap()
                    .as_bytes(),
                )
                .unwrap();
            let executable_path = if cfg!(windows) { "bin/plugin.exe" } else { "bin/plugin" };
            archive
                .start_file(format!("{CC_SWITCH_PLUGIN_ID}/{executable_path}"), options)
                .unwrap();
            archive.write_all(b"plugin").unwrap();
            archive.finish().unwrap();
        }

        install_cc_switch_plugin_zip(bytes.get_ref(), root.path()).unwrap();
        let status = cc_switch_plugin_status(root.path()).unwrap();
        assert!(status.installed);
        assert_eq!(status.version.as_deref(), Some("test"));
        assert!(crate::plugins::PluginRegistry::new(root.path().to_path_buf())
            .find_capability(CC_SWITCH_PLUGIN_CAPABILITY)
            .unwrap()
            .is_some());

        let status = uninstall_cc_switch_plugin(root.path()).unwrap();
        assert!(!status.installed);
        assert!(!root.path().join(CC_SWITCH_PLUGIN_ID).exists());
    }
}
