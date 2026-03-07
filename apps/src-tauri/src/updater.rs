use reqwest::blocking::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Manager;
use zip::ZipArchive;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const DEFAULT_UPDATE_REPO: &str = "qxcnm/Codex-Manager";
const PORTABLE_MARKER_FILE: &str = ".codexmanager-portable";
const PENDING_UPDATE_FILE: &str = "pending-update.json";
const USER_AGENT: &str = "CodexManager-Updater";

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResponse {
    repo: String,
    mode: String,
    is_portable: bool,
    has_update: bool,
    can_prepare: bool,
    current_version: String,
    latest_version: String,
    release_tag: String,
    release_name: Option<String>,
    published_at: Option<String>,
    reason: Option<String>,
    checked_at_unix_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePrepareResponse {
    prepared: bool,
    mode: String,
    is_portable: bool,
    release_tag: String,
    latest_version: String,
    asset_name: String,
    asset_path: String,
    downloaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateActionResponse {
    ok: bool,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingUpdate {
    mode: String,
    is_portable: bool,
    release_tag: String,
    latest_version: String,
    asset_name: String,
    asset_path: String,
    installer_path: Option<String>,
    staging_dir: Option<String>,
    prepared_at_unix_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusResponse {
    repo: String,
    mode: String,
    is_portable: bool,
    current_version: String,
    current_exe_path: String,
    portable_marker_path: String,
    pending: Option<PendingUpdate>,
    last_check: Option<UpdateCheckResponse>,
    last_error: Option<String>,
}

#[derive(Debug, Default)]
struct UpdaterState {
    last_check: Option<UpdateCheckResponse>,
    last_error: Option<String>,
}

struct ResolvedUpdateContext {
    check: UpdateCheckResponse,
    payload_asset: Option<GitHubAsset>,
}

static UPDATER_STATE: OnceLock<Mutex<UpdaterState>> = OnceLock::new();

fn updater_state() -> &'static Mutex<UpdaterState> {
    UPDATER_STATE.get_or_init(|| Mutex::new(UpdaterState::default()))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .unwrap_or(0)
}

fn resolve_update_repo() -> String {
    std::env::var("CODEXMANAGER_UPDATE_REPO")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_UPDATE_REPO.to_string())
}

fn normalize_version(input: &str) -> Result<Version, String> {
    let normalized = input.trim().trim_start_matches(['v', 'V']);
    Version::parse(normalized).map_err(|err| format!("版本号无效 '{input}'：{err}"))
}

fn current_exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|err| format!("解析当前可执行文件路径失败：{err}"))
}

fn current_mode_and_marker() -> Result<(String, bool, PathBuf, PathBuf), String> {
    let exe = current_exe_path()?;
    let exe_dir = exe
        .parent()
        .ok_or_else(|| "解析可执行文件所在目录失败".to_string())?
        .to_path_buf();
    let marker = exe_dir.join(PORTABLE_MARKER_FILE);
    let by_marker = marker.is_file();
    let by_exe_name = exe
        .file_name()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase().contains("-portable"))
        .unwrap_or(false);
    let is_portable = by_marker || by_exe_name;
    let mode = if is_portable { "portable" } else { "installer" }.to_string();
    Ok((mode, is_portable, exe, marker))
}

fn env_flag(name: &str) -> Option<bool> {
    let raw = std::env::var(name).ok()?;
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn should_include_prerelease_updates_with_override(
    current_version: &Version,
    override_value: Option<bool>,
) -> bool {
    override_value.unwrap_or_else(|| !current_version.pre.is_empty())
}

fn should_include_prerelease_updates(current_version: &Version) -> bool {
    should_include_prerelease_updates_with_override(
        current_version,
        env_flag("CODEXMANAGER_UPDATE_PRERELEASE"),
    )
}

fn http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|err| format!("创建 HTTP 客户端失败：{err}"))
}

fn resolve_github_token() -> Option<String> {
    for key in ["CODEXMANAGER_GITHUB_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn extract_tag_from_release_url(url: &str) -> Option<String> {
    let marker = "/releases/tag/";
    let (_, tail) = url.split_once(marker)?;
    let tag = tail
        .split(['?', '#', '/'])
        .next()
        .map(|v| v.trim())
        .unwrap_or("");
    if tag.is_empty() {
        None
    } else {
        Some(tag.to_string())
    }
}

fn normalize_release_asset_url(raw: &str, repo: &str) -> Option<String> {
    let href = raw.trim().replace("&amp;", "&");
    if href.is_empty() {
        return None;
    }

    let absolute = if href.starts_with("https://github.com/") {
        href
    } else if href.starts_with("http://github.com/") {
        href.replacen("http://", "https://", 1)
    } else if href.starts_with("//github.com/") {
        format!("https:{href}")
    } else if href.starts_with('/') {
        format!("https://github.com{href}")
    } else {
        return None;
    };

    let marker = format!("/{repo}/releases/download/");
    if absolute.contains(&marker) {
        Some(absolute)
    } else {
        None
    }
}

fn asset_name_from_download_url(url: &str) -> Option<String> {
    let without_fragment = url.split('#').next().unwrap_or(url);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let name = without_query.rsplit('/').next().unwrap_or("").trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn parse_release_assets_from_html(html: &str, repo: &str) -> Vec<GitHubAsset> {
    let mut assets = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = html;
    loop {
        let Some(idx) = cursor.find("href=\"") else {
            break;
        };
        cursor = &cursor[idx + 6..];
        let Some(end_idx) = cursor.find('"') else {
            break;
        };

        let href = &cursor[..end_idx];
        if let Some(url) = normalize_release_asset_url(href, repo) {
            if let Some(name) = asset_name_from_download_url(&url) {
                let key = name.to_ascii_lowercase();
                if seen.insert(key) {
                    assets.push(GitHubAsset {
                        name,
                        browser_download_url: url,
                    });
                }
            }
        }
        cursor = &cursor[end_idx + 1..];
    }
    assets
}

fn fetch_release_assets_from_expanded_fragment(
    client: &Client,
    repo: &str,
    tag: &str,
) -> Result<Vec<GitHubAsset>, String> {
    let url = format!("https://github.com/{repo}/releases/expanded_assets/{tag}");
    let html = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
        .send()
        .map_err(|err| format!("请求扩展资产列表失败：{err}"))?
        .error_for_status()
        .map_err(|err| format!("扩展资产列表响应异常：{err}"))?
        .text()
        .map_err(|err| format!("读取扩展资产列表失败：{err}"))?;
    Ok(parse_release_assets_from_html(&html, repo))
}

fn fetch_latest_release_via_html(client: &Client, repo: &str) -> Result<GitHubRelease, String> {
    let url = format!("https://github.com/{repo}/releases/latest");
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
        .send()
        .map_err(|err| format!("请求最新发布页跳转失败：{err}"))?
        .error_for_status()
        .map_err(|err| format!("最新发布页跳转响应异常：{err}"))?;

    let final_url = response.url().as_str().to_string();
    let tag = extract_tag_from_release_url(&final_url)
        .ok_or_else(|| format!("无法从 GitHub Releases 地址解析最新标签：{final_url}"))?;
    let html = response
        .text()
        .map_err(|err| format!("读取最新发布页失败：{err}"))?;
    let mut assets = parse_release_assets_from_html(&html, repo);
    if assets.is_empty() {
        if let Ok(expanded_assets) = fetch_release_assets_from_expanded_fragment(client, repo, &tag)
        {
            if !expanded_assets.is_empty() {
                assets = expanded_assets;
            }
        }
    }

    Ok(GitHubRelease {
        tag_name: tag,
        name: None,
        published_at: None,
        draft: false,
        prerelease: false,
        assets,
    })
}

fn select_release_for_channel(
    releases: Vec<GitHubRelease>,
    include_prerelease: bool,
) -> Result<GitHubRelease, String> {
    let mut selected: Option<(Version, GitHubRelease)> = None;

    for release in releases {
        if release.draft {
            continue;
        }
        if !include_prerelease && release.prerelease {
            continue;
        }

        let version = match normalize_version(&release.tag_name) {
            Ok(value) => value,
            Err(_) => continue,
        };

        match &selected {
            Some((best_version, _)) if version <= *best_version => {}
            _ => selected = Some((version, release)),
        }
    }

    selected.map(|(_, release)| release).ok_or_else(|| {
        if include_prerelease {
            "未找到可用的稳定版或预发布版本".to_string()
        } else {
            "未找到可用的稳定版发布".to_string()
        }
    })
}

fn fetch_latest_release(
    client: &Client,
    repo: &str,
    include_prerelease: bool,
) -> Result<GitHubRelease, String> {
    if !repo.contains('/') {
        return Err(format!("更新仓库配置无效 '{repo}'，应为 owner/repo 格式"));
    }
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=20");
    let mut req = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json");
    if let Some(token) = resolve_github_token() {
        req = req.bearer_auth(token);
    }

    let release = match req.send() {
        Ok(resp) => match resp.error_for_status() {
            Ok(ok_resp) => {
                let releases = ok_resp
                    .json::<Vec<GitHubRelease>>()
                    .map_err(|err| format!("解析发布列表失败：{err}"))?;
                select_release_for_channel(releases, include_prerelease)?
            }
            Err(api_err) => {
                if include_prerelease {
                    return Err(format!(
            "发布列表 API 请求失败（{api_err}）；预发布通道不支持 HTML 回退，请重试或配置 CODEXMANAGER_GITHUB_TOKEN"
          ));
                }
                fetch_latest_release_via_html(client, repo).map_err(|fallback_err| {
                    format!(
            "最新发布 API 请求失败（{api_err}）；回退解析发布页面也失败（{fallback_err}）"
          )
                })?
            }
        },
        Err(api_transport_err) => {
            if include_prerelease {
                return Err(format!(
          "发布列表请求失败（{api_transport_err}）；预发布通道不支持 HTML 回退，请重试或配置 CODEXMANAGER_GITHUB_TOKEN"
        ));
            }
            fetch_latest_release_via_html(client, repo).map_err(|fallback_err| {
                format!(
          "最新发布请求失败（{api_transport_err}）；回退解析发布页面也失败（{fallback_err}）"
        )
            })?
        }
    };

    Ok(release)
}

fn portable_asset_names_for_platform(latest_version: &str) -> Vec<String> {
    let v = latest_version.trim().trim_start_matches(['v', 'V']);
    if cfg!(target_os = "windows") {
        vec![
            "CodexManager-portable.exe".to_string(),
            format!("CodexManager-{v}-windows-portable.zip"),
            "CodexManager-windows-portable.zip".to_string(),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            format!("CodexManager-{v}-macos-portable.zip"),
            "CodexManager-macos-portable.zip".to_string(),
        ]
    } else {
        vec![
            format!("CodexManager-{v}-linux-portable.zip"),
            "CodexManager-linux-portable.zip".to_string(),
        ]
    }
}

fn select_payload_asset(
    mode: &str,
    latest_version: &str,
    assets: &[GitHubAsset],
) -> Option<GitHubAsset> {
    if mode == "portable" {
        let portable_names = portable_asset_names_for_platform(latest_version);
        for expected in portable_names {
            if let Some(asset) = assets
                .iter()
                .find(|asset| asset.name.eq_ignore_ascii_case(&expected))
            {
                return Some(asset.clone());
            }
        }
        return None;
    }

    if cfg!(target_os = "windows") {
        if let Some(exe) = assets.iter().find(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.ends_with(".exe") && !name.contains("portable")
        }) {
            return Some(exe.clone());
        }
        return assets
            .iter()
            .find(|asset| {
                let name = asset.name.to_ascii_lowercase();
                name.ends_with(".msi") && !name.contains("portable")
            })
            .cloned();
    }

    if cfg!(target_os = "macos") {
        return assets
            .iter()
            .find(|asset| asset.name.to_ascii_lowercase().ends_with(".dmg"))
            .cloned();
    }

    if let Some(appimage) = assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with(".appimage"))
    {
        return Some(appimage.clone());
    }
    if let Some(deb) = assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with(".deb"))
    {
        return Some(deb.clone());
    }
    assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with(".rpm"))
        .cloned()
}

fn resolve_update_context() -> Result<ResolvedUpdateContext, String> {
    let repo = resolve_update_repo();
    let (mode, is_portable, _, _) = current_mode_and_marker()?;
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let current_semver = normalize_version(&current_version)?;
    let include_prerelease = should_include_prerelease_updates(&current_semver);

    let client = http_client()?;
    let release = fetch_latest_release(&client, &repo, include_prerelease)?;
    let latest_semver = normalize_version(&release.tag_name)?;
    let has_update = latest_semver > current_semver;

    let payload_asset = select_payload_asset(&mode, &latest_semver.to_string(), &release.assets);
    let can_prepare = has_update && payload_asset.is_some();
    let fetched_by_fallback = release.assets.is_empty();

    let reason = if !has_update {
        Some("当前版本已是最新".to_string())
    } else if fetched_by_fallback {
        Some(
      "已在 GitHub Releases 页面发现新版本，但发布资产元数据不可用（可能是 GitHub API 速率限制或页面解析偏移）；可设置 CODEXMANAGER_GITHUB_TOKEN 提高一键更新稳定性".to_string(),
    )
    } else if payload_asset.is_none() {
        Some("未找到当前平台/运行模式对应的发布资产".to_string())
    } else {
        None
    };

    let check = UpdateCheckResponse {
        repo,
        mode,
        is_portable,
        has_update,
        can_prepare,
        current_version,
        latest_version: latest_semver.to_string(),
        release_tag: release.tag_name.clone(),
        release_name: release.name.clone(),
        published_at: release.published_at.clone(),
        reason,
        checked_at_unix_secs: now_unix_secs(),
    };

    Ok(ResolvedUpdateContext {
        check,
        payload_asset,
    })
}

fn set_last_check(check: UpdateCheckResponse) {
    if let Ok(mut guard) = updater_state().lock() {
        guard.last_check = Some(check);
        guard.last_error = None;
    }
}

fn set_last_error(message: String) {
    if let Ok(mut guard) = updater_state().lock() {
        guard.last_error = Some(message);
    }
}

fn updates_root_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|_| "未找到应用数据目录".to_string())?;
    root.push("updates");
    fs::create_dir_all(&root).map_err(|err| format!("创建更新目录失败：{err}"))?;
    Ok(root)
}

fn pending_update_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(updates_root_dir(app)?.join(PENDING_UPDATE_FILE))
}

fn read_pending_update(app: &tauri::AppHandle) -> Result<Option<PendingUpdate>, String> {
    let path = pending_update_path(app)?;
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|err| format!("读取待安装更新信息失败：{err}"))?;
    let parsed = serde_json::from_slice::<PendingUpdate>(&bytes)
        .map_err(|err| format!("解析待安装更新信息失败：{err}"))?;
    Ok(Some(parsed))
}

fn write_pending_update(app: &tauri::AppHandle, pending: &PendingUpdate) -> Result<(), String> {
    let path = pending_update_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("创建待安装信息目录失败：{err}"))?;
    }
    let bytes = serde_json::to_vec_pretty(pending)
        .map_err(|err| format!("序列化待安装更新信息失败：{err}"))?;
    fs::write(&path, bytes).map_err(|err| format!("写入待安装更新信息失败：{err}"))
}

fn clear_pending_update(app: &tauri::AppHandle) -> Result<(), String> {
    let path = pending_update_path(app)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|err| format!("删除待安装更新信息失败：{err}"))?;
    }
    Ok(())
}

fn sanitize_tag(tag: &str) -> String {
    let out: String = tag
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

fn download_to_file(client: &Client, url: &str, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("创建下载目录失败：{err}"))?;
    }
    let mut resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .map_err(|err| format!("发起下载请求失败：{err}"))?
        .error_for_status()
        .map_err(|err| format!("下载响应异常：{err}"))?;

    let mut file = File::create(target).map_err(|err| format!("创建文件失败：{err}"))?;
    std::io::copy(&mut resp, &mut file).map_err(|err| format!("写入文件失败：{err}"))?;
    file.flush()
        .map_err(|err| format!("刷新文件缓冲区失败：{err}"))
}

fn portable_executable_candidates() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["CodexManager-portable.exe", "CodexManager.exe"]
    } else if cfg!(target_os = "macos") {
        &[
            "CodexManager-portable.app",
            "CodexManager.app",
            "CodexManager",
        ]
    } else {
        &["CodexManager-portable", "CodexManager"]
    }
}

fn resolve_portable_restart_exe(
    staging_dir: &Path,
    current_exe_name: &str,
) -> Result<String, String> {
    if staging_dir.join(current_exe_name).is_file() {
        return Ok(current_exe_name.to_string());
    }

    for candidate in portable_executable_candidates() {
        if staging_dir.join(candidate).is_file() {
            return Ok((*candidate).to_string());
        }
    }

    Err(format!(
        "便携包无效：暂存目录中未找到可执行文件，期望名称之一为 [{}]",
        portable_executable_candidates().join(", ")
    ))
}

fn extract_zip_archive(zip_path: &Path, target_dir: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|err| format!("打开 ZIP 包失败：{err}"))?;
    let mut archive = ZipArchive::new(file).map_err(|err| format!("读取 ZIP 包失败：{err}"))?;

    for idx in 0..archive.len() {
        let mut entry = archive
            .by_index(idx)
            .map_err(|err| format!("读取 ZIP 条目失败：{err}"))?;
        let Some(relative_path) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        let out_path = target_dir.join(relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|err| format!("创建目录失败：{err}"))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("创建父目录失败：{err}"))?;
        }
        let mut out_file = File::create(&out_path).map_err(|err| format!("创建文件失败：{err}"))?;
        std::io::copy(&mut entry, &mut out_file).map_err(|err| format!("解压文件失败：{err}"))?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(mode));
        }
    }

    Ok(())
}

fn stage_portable_payload(
    payload_path: &Path,
    payload_name: &str,
    staging_dir: &Path,
) -> Result<(), String> {
    let extension = payload_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if extension == "zip" {
        return extract_zip_archive(payload_path, staging_dir);
    }

    let file_name = Path::new(payload_name)
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("无法解析便携更新文件名：{payload_name}"))?;
    let target_path = staging_dir.join(file_name);
    fs::copy(payload_path, &target_path).map_err(|err| format!("复制便携更新文件失败：{err}"))?;

    #[cfg(unix)]
    {
        let _ = fs::set_permissions(&target_path, fs::Permissions::from_mode(0o755));
    }

    Ok(())
}

fn prepare_update_impl(app: &tauri::AppHandle) -> Result<UpdatePrepareResponse, String> {
    let context = resolve_update_context()?;
    set_last_check(context.check.clone());

    if !context.check.has_update {
        return Err("当前版本已是最新".to_string());
    }
    if !context.check.can_prepare {
        return Err(context
            .check
            .reason
            .clone()
            .unwrap_or_else(|| "更新尚未准备就绪".to_string()));
    }

    let payload_asset = context
        .payload_asset
        .clone()
        .ok_or_else(|| "缺少可下载安装的发布资产".to_string())?;
    let client = http_client()?;
    let release_dir = updates_root_dir(app)?.join(sanitize_tag(&context.check.release_tag));
    fs::create_dir_all(&release_dir).map_err(|err| format!("创建发布目录失败：{err}"))?;

    let payload_path = release_dir.join(&payload_asset.name);
    download_to_file(&client, &payload_asset.browser_download_url, &payload_path)?;

    let mut pending = PendingUpdate {
        mode: context.check.mode.clone(),
        is_portable: context.check.is_portable,
        release_tag: context.check.release_tag.clone(),
        latest_version: context.check.latest_version.clone(),
        asset_name: payload_asset.name.clone(),
        asset_path: payload_path.display().to_string(),
        installer_path: None,
        staging_dir: None,
        prepared_at_unix_secs: now_unix_secs(),
    };

    if context.check.mode == "portable" {
        let staging_dir = release_dir.join("staging");
        if staging_dir.is_dir() {
            fs::remove_dir_all(&staging_dir).map_err(|err| format!("清理暂存目录失败：{err}"))?;
        }
        fs::create_dir_all(&staging_dir).map_err(|err| format!("创建暂存目录失败：{err}"))?;
        stage_portable_payload(&payload_path, &payload_asset.name, &staging_dir)?;
        let current_exe_name = current_exe_path()?
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "解析当前可执行文件名失败".to_string())?
            .to_string();
        let _ = resolve_portable_restart_exe(&staging_dir, &current_exe_name)?;
        pending.staging_dir = Some(staging_dir.display().to_string());
    } else {
        pending.installer_path = Some(payload_path.display().to_string());
    }

    write_pending_update(app, &pending)?;

    Ok(UpdatePrepareResponse {
        prepared: true,
        mode: context.check.mode,
        is_portable: context.check.is_portable,
        release_tag: context.check.release_tag,
        latest_version: context.check.latest_version,
        asset_name: pending.asset_name,
        asset_path: pending.asset_path,
        downloaded: true,
    })
}

fn script_dir_from_pending(
    pending: &PendingUpdate,
    app: &tauri::AppHandle,
) -> Result<PathBuf, String> {
    let asset_path = PathBuf::from(&pending.asset_path);
    if let Some(parent) = asset_path.parent() {
        return Ok(parent.to_path_buf());
    }
    updates_root_dir(app)
}

fn spawn_portable_apply_worker(
    script_dir: &Path,
    target_dir: &Path,
    staging_dir: &Path,
    exe_name: &str,
    pending_path: &Path,
    pid_to_wait: u32,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let script_path = script_dir.join("apply-portable-update.ps1");
        let script = r#"
param(
  [Parameter(Mandatory=$true)][string]$TargetDir,
  [Parameter(Mandatory=$true)][string]$StagingDir,
  [Parameter(Mandatory=$true)][string]$ExeName,
  [Parameter(Mandatory=$true)][string]$PendingFile,
  [Parameter(Mandatory=$true)][int]$PidToWait
)
$ErrorActionPreference = "Stop"
for ($i = 0; $i -lt 240; $i++) {
  if (-not (Get-Process -Id $PidToWait -ErrorAction SilentlyContinue)) { break }
  Start-Sleep -Milliseconds 500
}
Get-ChildItem -LiteralPath $StagingDir -Force | ForEach-Object {
  Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $TargetDir $_.Name) -Recurse -Force
}
if (Test-Path -LiteralPath $PendingFile) {
  Remove-Item -LiteralPath $PendingFile -Force -ErrorAction SilentlyContinue
}
Start-Process -FilePath (Join-Path $TargetDir $ExeName) | Out-Null
"#;
        fs::write(&script_path, script).map_err(|err| format!("写入更新应用脚本失败：{err}"))?;

        let args = vec![
            "-TargetDir".to_string(),
            target_dir.display().to_string(),
            "-StagingDir".to_string(),
            staging_dir.display().to_string(),
            "-ExeName".to_string(),
            exe_name.to_string(),
            "-PendingFile".to_string(),
            pending_path.display().to_string(),
            "-PidToWait".to_string(),
            pid_to_wait.to_string(),
        ];

        let try_spawn = |shell: &str| -> Result<(), String> {
            let mut cmd = Command::new(shell);
            cmd.arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(&script_path)
                .args(&args);
            cmd.creation_flags(CREATE_NO_WINDOW);
            cmd.spawn()
                .map(|_| ())
                .map_err(|err| format!("启动 {shell} 失败：{err}"))
        };

        if try_spawn("powershell.exe").is_ok() {
            return Ok(());
        }
        return try_spawn("pwsh.exe");
    }

    #[cfg(not(target_os = "windows"))]
    {
        let script_path = script_dir.join("apply-portable-update.sh");
        let script = r#"#!/usr/bin/env sh
TARGET_DIR="$1"
STAGING_DIR="$2"
EXE_NAME="$3"
PENDING_FILE="$4"
PID_TO_WAIT="$5"

i=0
while kill -0 "$PID_TO_WAIT" 2>/dev/null && [ "$i" -lt 240 ]; do
  i=$((i + 1))
  sleep 0.5
done

cp -Rf "$STAGING_DIR"/. "$TARGET_DIR"/
rm -f "$PENDING_FILE"
chmod +x "$TARGET_DIR/$EXE_NAME" 2>/dev/null || true
"$TARGET_DIR/$EXE_NAME" >/dev/null 2>&1 &
"#;
        fs::write(&script_path, script).map_err(|err| format!("写入更新应用脚本失败：{err}"))?;

        #[cfg(unix)]
        {
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
                .map_err(|err| format!("设置更新应用脚本权限失败：{err}"))?;
        }

        Command::new("sh")
            .arg(&script_path)
            .arg(target_dir)
            .arg(staging_dir)
            .arg(exe_name)
            .arg(pending_path)
            .arg(pid_to_wait.to_string())
            .spawn()
            .map_err(|err| format!("启动更新应用脚本失败：{err}"))?;
        Ok(())
    }
}

fn schedule_app_exit(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(280));
        app.exit(0);
    });
}

fn launch_installer(installer_path: &Path) -> Result<(), String> {
    if !installer_path.is_file() {
        return Err(format!("未找到安装包：{}", installer_path.display()));
    }

    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new(installer_path);
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.spawn()
            .map_err(|err| format!("启动安装包失败：{err}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(installer_path)
            .spawn()
            .map_err(|err| format!("打开安装包失败：{err}"))?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let ext = installer_path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "appimage" {
            #[cfg(unix)]
            {
                let _ = fs::set_permissions(installer_path, fs::Permissions::from_mode(0o755));
            }
            Command::new(installer_path)
                .spawn()
                .map_err(|err| format!("启动 AppImage 失败：{err}"))?;
            return Ok(());
        }

        Command::new("xdg-open")
            .arg(installer_path)
            .spawn()
            .map_err(|err| format!("打开安装包失败：{err}"))?;
        Ok(())
    }
}

#[tauri::command]
pub async fn app_update_check() -> Result<UpdateCheckResponse, String> {
    let task = tauri::async_runtime::spawn_blocking(resolve_update_context);
    match task.await {
        Ok(Ok(context)) => {
            set_last_check(context.check.clone());
            Ok(context.check)
        }
        Ok(Err(err)) => {
            set_last_error(err.clone());
            Err(err)
        }
        Err(err) => {
            let message = format!("app_update_check 任务失败：{err}");
            set_last_error(message.clone());
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn app_update_prepare(app: tauri::AppHandle) -> Result<UpdatePrepareResponse, String> {
    let app_handle = app.clone();
    let task = tauri::async_runtime::spawn_blocking(move || prepare_update_impl(&app_handle));
    match task.await {
        Ok(Ok(result)) => {
            if let Ok(mut guard) = updater_state().lock() {
                guard.last_error = None;
            }
            Ok(result)
        }
        Ok(Err(err)) => {
            set_last_error(err.clone());
            Err(err)
        }
        Err(err) => {
            let message = format!("app_update_prepare 任务失败：{err}");
            set_last_error(message.clone());
            Err(message)
        }
    }
}

#[tauri::command]
pub fn app_update_apply_portable(app: tauri::AppHandle) -> Result<UpdateActionResponse, String> {
    let pending = read_pending_update(&app)?
        .ok_or_else(|| "未找到已准备更新，请先调用 app_update_prepare".to_string())?;

    if pending.mode != "portable" {
        return Err("已准备更新并非便携模式".to_string());
    }

    let staging_dir = PathBuf::from(
        pending
            .staging_dir
            .as_ref()
            .ok_or_else(|| "便携更新缺少暂存目录".to_string())?,
    );
    if !staging_dir.is_dir() {
        return Err(format!("未找到暂存目录：{}", staging_dir.display()));
    }

    let exe_path = current_exe_path()?;
    let target_dir = exe_path
        .parent()
        .ok_or_else(|| "解析目标应用目录失败".to_string())?
        .to_path_buf();
    let exe_name = exe_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "解析当前可执行文件名失败".to_string())?
        .to_string();
    let restart_exe_name = resolve_portable_restart_exe(&staging_dir, &exe_name)?;
    let pending_path = pending_update_path(&app)?;
    let script_dir = script_dir_from_pending(&pending, &app)?;
    let pid = std::process::id();

    spawn_portable_apply_worker(
        &script_dir,
        &target_dir,
        &staging_dir,
        &restart_exe_name,
        &pending_path,
        pid,
    )?;

    schedule_app_exit(app);
    Ok(UpdateActionResponse {
        ok: true,
        message: "便携更新已就绪，应用将重启以完成替换".to_string(),
    })
}

#[tauri::command]
pub fn app_update_launch_installer(app: tauri::AppHandle) -> Result<UpdateActionResponse, String> {
    let pending = read_pending_update(&app)?
        .ok_or_else(|| "未找到已准备更新，请先调用 app_update_prepare".to_string())?;
    if pending.mode != "installer" {
        return Err("已准备更新并非安装包模式".to_string());
    }

    let installer_path = PathBuf::from(
        pending
            .installer_path
            .as_ref()
            .ok_or_else(|| "待安装更新中缺少安装包路径".to_string())?,
    );

    launch_installer(&installer_path)?;
    clear_pending_update(&app)?;

    Ok(UpdateActionResponse {
        ok: true,
        message: format!("已启动安装包：{}", installer_path.display()),
    })
}

#[tauri::command]
pub fn app_update_status(app: tauri::AppHandle) -> Result<UpdateStatusResponse, String> {
    let repo = resolve_update_repo();
    let (mode, is_portable, exe_path, marker_path) = current_mode_and_marker()?;
    let pending = read_pending_update(&app)?;
    let (last_check, last_error) = if let Ok(guard) = updater_state().lock() {
        (guard.last_check.clone(), guard.last_error.clone())
    } else {
        (None, Some("读取更新器状态锁失败".to_string()))
    };

    Ok(UpdateStatusResponse {
        repo,
        mode,
        is_portable,
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        current_exe_path: exe_path.display().to_string(),
        portable_marker_path: marker_path.display().to_string(),
        pending,
        last_check,
        last_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prerelease_channel_defaults_follow_current_version() {
        let stable = Version::parse("0.1.6").expect("stable version");
        let beta = Version::parse("0.1.7-beta.1").expect("beta version");

        assert!(!should_include_prerelease_updates_with_override(
            &stable, None
        ));
        assert!(should_include_prerelease_updates_with_override(&beta, None));
        assert!(should_include_prerelease_updates_with_override(
            &stable,
            Some(true)
        ));
        assert!(!should_include_prerelease_updates_with_override(
            &beta,
            Some(false)
        ));
    }

    #[test]
    fn portable_asset_names_include_current_workflow_artifact() {
        let names = portable_asset_names_for_platform("0.1.6");
        if cfg!(target_os = "windows") {
            assert!(names.iter().any(|name| name == "CodexManager-portable.exe"));
        } else if cfg!(target_os = "linux") {
            assert!(names
                .iter()
                .any(|name| name == "CodexManager-linux-portable.zip"));
        } else if cfg!(target_os = "macos") {
            assert!(names
                .iter()
                .any(|name| name == "CodexManager-macos-portable.zip"));
        }
    }

    #[test]
    fn release_selection_respects_channel() {
        let releases = vec![
            GitHubRelease {
                tag_name: "v0.1.7-beta.1".to_string(),
                name: None,
                published_at: None,
                draft: false,
                prerelease: true,
                assets: vec![],
            },
            GitHubRelease {
                tag_name: "v0.1.6".to_string(),
                name: None,
                published_at: None,
                draft: false,
                prerelease: false,
                assets: vec![],
            },
        ];

        let stable = select_release_for_channel(releases.clone(), false).expect("stable release");
        let prerelease = select_release_for_channel(releases, true).expect("prerelease release");

        assert_eq!(stable.tag_name, "v0.1.6");
        assert_eq!(prerelease.tag_name, "v0.1.7-beta.1");
    }
}
