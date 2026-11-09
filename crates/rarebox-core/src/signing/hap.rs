//! 读取 HAP/HSP/APP 内的 `module.json`，提取包名、版本和权限。
//!
//! 签名需要 bundle name 来申请 Profile；ACL 需要 `module.requestPermissions`
//! 与 AGC 白名单求交集。APP 包是 HAP 的 ZIP 容器，这里会逐个解析内嵌 HAP。

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use serde_json::Value;
use zip::ZipArchive;

use super::model::{HapMeta, is_acl_permission};

const MAX_INNER_HAP: u64 = 512 * 1024 * 1024;

pub fn parse(path: &Path) -> Result<HapMeta> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "hap" | "hsp" => parse_single(path, false),
        "app" => parse_app(path),
        _ => bail!("仅支持 .hap / .hsp / .app 文件"),
    }
}

fn parse_single(path: &Path, inner: bool) -> Result<HapMeta> {
    let file = File::open(path).with_context(|| format!("无法打开 {}", path.display()))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .with_context(|| format!("{} 不是有效的 ZIP/HAP", path.display()))?;
    let module = read_module_json(&mut archive)
        .with_context(|| format!("{} 中缺少可解析的 module.json", path.display()))?;
    let mut meta = meta_from_module(&module, path)?;
    if inner {
        meta.path = path.to_path_buf();
    }
    Ok(meta)
}

fn parse_app(path: &Path) -> Result<HapMeta> {
    let file = File::open(path).with_context(|| format!("无法打开 {}", path.display()))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .with_context(|| format!("{} 不是有效的 APP 包", path.display()))?;

    let mut bundle_from_pack = String::new();
    if let Ok(mut entry) = archive.by_name("pack.info") {
        let mut raw = String::new();
        if entry.read_to_string(&mut raw).is_ok() {
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                bundle_from_pack = value
                    .pointer("/summary/app/bundleName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
        }
    }

    let mut permissions = BTreeSet::new();
    let mut meta: Option<HapMeta> = None;
    let mut inner_haps = 0usize;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("无法读取 {} 的第 {index} 个条目", path.display()))?;
        let name = entry.name().to_string();
        if !name.to_ascii_lowercase().ends_with(".hap") {
            continue;
        }
        if entry.size() > MAX_INNER_HAP {
            bail!("APP 内的 {name} 过大，拒绝解析");
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        let mut inner = ZipArchive::new(Cursor::new(bytes))
            .with_context(|| format!("APP 内的 {name} 不是有效 HAP"))?;
        let module = read_module_json(&mut inner)
            .with_context(|| format!("APP 内的 {name} 缺少 module.json"))?;
        let parsed = meta_from_module(&module, path)?;
        permissions.extend(parsed.permissions.iter().cloned());
        if meta.is_none() {
            meta = Some(parsed);
        }
        inner_haps += 1;
    }

    let mut meta = meta.ok_or_else(|| anyhow::anyhow!("APP 包中没有找到 HAP"))?;
    meta.inner_haps = inner_haps;
    meta.permissions = permissions.into_iter().collect();
    meta.acl = meta
        .permissions
        .iter()
        .filter(|permission| is_acl_permission(permission))
        .cloned()
        .collect();
    if !bundle_from_pack.is_empty() {
        meta.bundle_name = bundle_from_pack;
    }
    Ok(meta)
}

fn read_module_json<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>) -> Result<Value> {
    let mut entry = archive.by_name("module.json")?;
    let mut raw = String::new();
    entry.read_to_string(&mut raw)?;
    let value = serde_json::from_str::<Value>(&raw).context("module.json 不是有效 JSON")?;
    Ok(value)
}

fn meta_from_module(value: &Value, path: &Path) -> Result<HapMeta> {
    let app = value.get("app").context("module.json 缺少 app 节点")?;
    let module = value.get("module").unwrap_or(&Value::Null);
    let bundle_name = app
        .get("bundleName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if bundle_name.is_empty() {
        bail!("module.json 缺少 app.bundleName");
    }
    let permissions: Vec<String> = module
        .get("requestPermissions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let acl = permissions
        .iter()
        .filter(|permission| is_acl_permission(permission))
        .cloned()
        .collect();
    let device_types = module
        .get("deviceTypes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let version_code = app
        .get("versionCode")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let version_name = app
        .get("versionName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let min_api = app
        .get("minAPIVersion")
        .and_then(Value::as_u64)
        .map(|value| value as u32);
    Ok(HapMeta {
        path: path.to_path_buf(),
        bundle_name,
        version_name,
        version_code,
        module_name: module
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        module_type: module
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        permissions,
        acl,
        min_api,
        device_types,
        inner_haps: 0,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default();
            for (name, bytes) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn extracts_bundle_and_acl() {
        let module = br#"{
            "app": {"bundleName": "com.example.app", "versionCode": 42, "versionName": "1.2.3", "minAPIVersion": 12},
            "module": {
                "name": "entry", "type": "entry", "deviceTypes": ["phone"],
                "requestPermissions": [
                    {"name": "ohos.permission.READ_CONTACTS"},
                    {"name": "ohos.permission.INTERNET"}
                ]
            }
        }"#;
        let bytes = make_zip(&[("module.json", module)]);
        let dir = std::env::temp_dir().join(format!("rarebox-hap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("entry.hap");
        std::fs::write(&path, bytes).unwrap();
        let meta = parse(&path).unwrap();
        assert_eq!(meta.bundle_name, "com.example.app");
        assert_eq!(meta.version_code, 42);
        assert_eq!(meta.version_name, "1.2.3");
        assert_eq!(meta.min_api, Some(12));
        assert_eq!(meta.permissions.len(), 2);
        assert_eq!(meta.acl, vec!["ohos.permission.READ_CONTACTS".to_string()]);
        assert_eq!(meta.inner_haps, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unions_app_inner_hap_permissions() {
        let first = make_zip(&[(
            "module.json",
            br#"{"app":{"bundleName":"com.example.app"},"module":{"name":"entry","requestPermissions":[{"name":"ohos.permission.READ_CONTACTS"}]}}"#,
        )]);
        let second = make_zip(&[(
            "module.json",
            br#"{"app":{"bundleName":"com.example.app"},"module":{"name":"feature","requestPermissions":[{"name":"ohos.permission.INPUT_MONITORING"}]}}"#,
        )]);
        let pack = br#"{"summary":{"app":{"bundleName":"com.example.app"}}}"#;
        let bytes = make_zip(&[
            ("pack.info", pack),
            ("entry.hap", &first),
            ("feature.hap", &second),
        ]);
        let dir = std::env::temp_dir().join(format!("rarebox-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bundle.app");
        std::fs::write(&path, bytes).unwrap();
        let meta = parse(&path).unwrap();
        assert_eq!(meta.inner_haps, 2);
        assert_eq!(meta.acl.len(), 2);
        assert_eq!(meta.bundle_name, "com.example.app");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
