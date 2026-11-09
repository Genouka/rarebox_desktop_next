//! APK 安装：把本地 APK 按包名分组（含 split 分包），并生成 `adb install` 参数。
//!
//! 分包 APK 必须与主 APK 用同一条 `install-multiple` 命令一起提交，
//! 因此分组与参数拼装都在这里完成，界面层只负责选择文件与展示结果。

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::adb::{AdbClient, AdbOutput};
use crate::apk;

/// APK 里与元数据解析相关的固定条目。
const ANDROID_MANIFEST: &str = "AndroidManifest.xml";
const RESOURCES: &str = "resources.arsc";
/// 单个条目的读取上限：`resources.arsc` 可能有几 MB，但不必为损坏文件分配更多。
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

/// `adb install` 的可选参数。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct InstallOptions {
    /// `-r`：覆盖已安装的应用。
    pub replace: bool,
    /// `-d`：允许安装比设备上更低的版本。
    pub downgrade: bool,
    /// `-g`：授予全部运行时权限。
    pub grant_permissions: bool,
    /// `-t`：允许安装带 `testOnly` 标记的 APK。
    pub allow_test: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            replace: true,
            downgrade: false,
            grant_permissions: false,
            allow_test: false,
        }
    }
}

impl InstallOptions {
    pub fn get(&self, toggle: InstallToggle) -> bool {
        match toggle {
            InstallToggle::Replace => self.replace,
            InstallToggle::Downgrade => self.downgrade,
            InstallToggle::GrantPermissions => self.grant_permissions,
            InstallToggle::AllowTest => self.allow_test,
        }
    }

    pub fn set(&mut self, toggle: InstallToggle, value: bool) {
        match toggle {
            InstallToggle::Replace => self.replace = value,
            InstallToggle::Downgrade => self.downgrade = value,
            InstallToggle::GrantPermissions => self.grant_permissions = value,
            InstallToggle::AllowTest => self.allow_test = value,
        }
    }

    /// 传给 adb 的参数。降级安装必须先允许覆盖，所以 `-d` 会带上 `-r`。
    pub fn flags(&self) -> Vec<&'static str> {
        let mut flags = Vec::with_capacity(4);
        if self.replace || self.downgrade {
            flags.push("-r");
        }
        if self.downgrade {
            flags.push("-d");
        }
        if self.grant_permissions {
            flags.push("-g");
        }
        if self.allow_test {
            flags.push("-t");
        }
        flags
    }
}

/// 一个可以在界面上切换的安装参数。
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum InstallToggle {
    Replace,
    Downgrade,
    GrantPermissions,
    AllowTest,
}

impl InstallToggle {
    /// 界面上按顺序展示的全部参数。
    pub const ALL: [InstallToggle; 4] = [
        InstallToggle::Replace,
        InstallToggle::Downgrade,
        InstallToggle::GrantPermissions,
        InstallToggle::AllowTest,
    ];

    pub fn label(self) -> &'static str {
        match self {
            InstallToggle::Replace => "覆盖安装",
            InstallToggle::Downgrade => "允许降级",
            InstallToggle::GrantPermissions => "授予全部权限",
            InstallToggle::AllowTest => "允许测试包",
        }
    }

    /// 该参数对应的 adb 短选项。
    pub fn flag(self) -> &'static str {
        match self {
            InstallToggle::Replace => "-r",
            InstallToggle::Downgrade => "-d",
            InstallToggle::GrantPermissions => "-g",
            InstallToggle::AllowTest => "-t",
        }
    }

    /// 胶囊上的文案，带上 adb 参数，省掉额外解释。
    pub fn chip_label(self) -> String {
        format!("{} {}", self.label(), self.flag())
    }
}

/// 从本地 APK 中读出的、用于分组的信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApkFile {
    pub path: PathBuf,
    /// manifest 的 `package` 属性。
    pub package: Option<String>,
    /// manifest 的 `split` 属性；主 APK 为 `None`。
    pub split: Option<String>,
    pub label: Option<String>,
    pub version_code: Option<u64>,
}

/// 同一个包名下的全部本地 APK。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApkGroup {
    pub package: String,
    pub label: Option<String>,
    pub version_code: Option<u64>,
    /// 主 APK；分包安装时它必须排在参数列表最前面。
    pub base: Option<PathBuf>,
    pub splits: Vec<PathBuf>,
}

impl ApkGroup {
    pub fn is_split_install(&self) -> bool {
        !self.splits.is_empty()
    }

    pub fn apk_count(&self) -> usize {
        self.splits.len() + usize::from(self.base.is_some())
    }

    /// 要提交给 adb 的文件，主 APK 在前。
    pub fn files(&self) -> Vec<PathBuf> {
        self.base
            .iter()
            .chain(self.splits.iter())
            .cloned()
            .collect()
    }

    /// 展示名称：应用名称（包名），没有应用名称时只用包名。
    pub fn title(&self) -> String {
        match self
            .label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
        {
            Some(label) => format!("{label}（{}）", self.package),
            None => self.package.clone(),
        }
    }

    /// 安装方式的简短说明。
    pub fn kind_label(&self) -> String {
        if self.is_split_install() {
            format!("分包安装 · {} 个 APK", self.apk_count())
        } else {
            "单个 APK".to_string()
        }
    }
}

/// 一次安装的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallReport {
    /// 安装目标，例如 `微信（com.tencent.mm）` 或出错的文件名。
    pub title: String,
    /// 结果说明：成功时是 adb 的结论，失败时是原因与建议。
    pub detail: String,
    pub success: bool,
}

impl InstallReport {
    fn failed(title: String, detail: String) -> Self {
        Self {
            title,
            detail,
            success: false,
        }
    }
}

/// 一批 APK 的安装方案。
#[derive(Clone, Debug, Default)]
pub struct InstallPlan {
    /// 待安装的应用，按选择顺序排列。
    pub groups: Vec<ApkGroup>,
    /// 还没开始安装就已确定失败的 APK（读不出包名、缺少主 APK 等）。
    pub failures: Vec<InstallReport>,
}

impl InstallPlan {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.failures.is_empty()
    }
}

/// 一批安装的完整结果。
#[derive(Clone, Debug, Default)]
pub struct InstallBatch {
    pub reports: Vec<InstallReport>,
}

impl InstallBatch {
    pub fn succeeded(&self) -> usize {
        self.reports.iter().filter(|report| report.success).count()
    }

    pub fn failed(&self) -> usize {
        self.reports.iter().filter(|report| !report.success).count()
    }
}

/// 读取本地 APK 的 manifest 与资源表，得到分组需要的信息。
pub fn read_apk(path: &Path) -> Result<ApkFile> {
    let file = File::open(path).with_context(|| format!("无法读取 {}", path.display()))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .with_context(|| format!("{} 不是有效的 APK（zip 结构无法解析）", display_name(path)))?;
    let manifest = read_entry(&mut archive, ANDROID_MANIFEST)?;
    // 资源表只用于解析应用名称，缺失不影响分组。
    let resources = read_entry(&mut archive, RESOURCES).ok().flatten();
    let metadata = apk::metadata(manifest.as_deref(), resources.as_deref());
    Ok(ApkFile {
        path: path.to_path_buf(),
        package: metadata.package,
        split: metadata.split,
        label: metadata.label,
        version_code: metadata.version_code,
    })
}

/// 读取 APK 内的一个条目；条目不存在时返回 `None`。
fn read_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Option<Vec<u8>>> {
    let entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("无法读取 APK 内的 {name}")),
    };
    let mut bytes = Vec::with_capacity(entry.size().min(MAX_ENTRY_BYTES) as usize);
    entry
        .take(MAX_ENTRY_BYTES)
        .read_to_end(&mut bytes)
        .with_context(|| format!("无法读取 APK 内的 {name}"))?;
    Ok(Some(bytes))
}

/// 把选中的 APK 按包名分组，得到安装方案。
///
/// 同一包名下的主 APK 与全部分包会合成一组；读不出包名的文件、
/// 只有一个主 APK 却重复选择的文件、以及缺少主 APK 的分包都作为失败项返回。
pub fn plan_installs(paths: &[PathBuf]) -> InstallPlan {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut apks = Vec::with_capacity(paths.len());
    let mut failures = Vec::new();

    for path in paths {
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen.insert(key) {
            continue;
        }
        match read_apk(path) {
            Ok(apk) if apk.package.is_some() => apks.push(apk),
            Ok(_) => failures.push(InstallReport::failed(
                display_name(path),
                "AndroidManifest.xml 里没有包名，无法确定安装目标".to_string(),
            )),
            Err(err) => failures.push(InstallReport::failed(
                display_name(path),
                format!("{err:#}"),
            )),
        }
    }

    let mut plan = group_apks(apks);
    failures.append(&mut plan.failures);
    plan.failures = failures;
    plan
}

/// 纯分组逻辑：主 APK 与分包按 `package` 归并，并剔除无法确定安装目标的组合。
fn group_apks(apks: Vec<ApkFile>) -> InstallPlan {
    let mut plan = InstallPlan::default();
    let mut conflicts: Vec<usize> = Vec::new();

    for apk in apks {
        let package = apk.package.clone().unwrap_or_default();
        let index = match plan
            .groups
            .iter()
            .position(|group| group.package == package)
        {
            Some(index) => index,
            None => {
                plan.groups.push(ApkGroup {
                    package,
                    label: None,
                    version_code: None,
                    base: None,
                    splits: Vec::new(),
                });
                plan.groups.len() - 1
            }
        };
        let group = &mut plan.groups[index];
        if group.label.is_none() {
            group.label = apk.label.clone();
        }
        if group.version_code.is_none() {
            group.version_code = apk.version_code;
        }
        match &apk.split {
            Some(_) => group.splits.push(apk.path),
            None if group.base.is_none() => group.base = Some(apk.path),
            // 同一个包出现两个主 APK：多半是选了不同版本，交给用户确认而不是赌一个。
            None => conflicts.push(index),
        }
    }

    // 先按下标记录有问题的组，再统一转成失败项，避免边遍历边改。
    let mut kept = Vec::with_capacity(plan.groups.len());
    for (index, group) in plan.groups.into_iter().enumerate() {
        let title = group.title();
        if conflicts.contains(&index) {
            plan.failures.push(InstallReport::failed(
                title,
                "同一个应用有多个主 APK，无法确定要安装哪一个".to_string(),
            ));
            continue;
        }
        if group.base.is_none() {
            plan.failures.push(InstallReport::failed(
                title,
                format!(
                    "只选择了 {} 个分包，缺少主 APK：请把 base.apk 一起选中",
                    group.splits.len()
                ),
            ));
            continue;
        }
        kept.push(group);
    }
    plan.groups = kept;
    plan
}

/// 生成一条完整的安装命令参数（不含 `-s <serial>`）。
pub fn install_args(group: &ApkGroup, options: &InstallOptions) -> Vec<String> {
    let files = group.files();
    let mut args = Vec::with_capacity(files.len() + 5);
    args.push(
        if group.is_split_install() {
            "install-multiple"
        } else {
            "install"
        }
        .to_string(),
    );
    args.extend(options.flags().into_iter().map(str::to_string));
    args.extend(files.iter().map(|path| path.to_string_lossy().into_owned()));
    args
}

/// 在设备上安装一个分组，返回结果而不抛出错误：批量安装里一个失败不影响其余应用。
pub fn run_install(
    adb: &AdbClient,
    serial: &str,
    group: &ApkGroup,
    options: &InstallOptions,
) -> InstallReport {
    let args = install_args(group, options);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let count = group.apk_count();
    match adb.serial_command(serial, &refs) {
        Ok(output) => match install_outcome(&output) {
            Ok(detail) => {
                // 分包安装说明一共有几个 APK，单文件时 adb 的结论已经足够。
                let detail = if count > 1 {
                    format!("{detail} · {count} 个 APK")
                } else {
                    detail
                };
                InstallReport {
                    title: group.title(),
                    detail,
                    success: true,
                }
            }
            Err(detail) => InstallReport {
                title: group.title(),
                detail,
                success: false,
            },
        },
        Err(err) => InstallReport::failed(group.title(), format!("{err:#}")),
    }
}

/// 判断 `adb install` 的输出是否成功，失败时把原因翻译成可操作的建议。
pub fn install_outcome(output: &AdbOutput) -> Result<String, String> {
    let combined = output.combined();
    let text = combined.trim();
    let failed =
        !output.success() || text.contains("Failure") || text.contains("failed to install");
    if !failed {
        return Ok(last_line(text).unwrap_or("安装成功").to_string());
    }
    Err(describe_failure(text))
}

/// 失败原因与常见处理办法。
const FAILURE_HINTS: &[(&str, &str)] = &[
    (
        "INSTALL_FAILED_VERSION_DOWNGRADE",
        "设备上已安装更高的版本，勾选「允许降级」后重试",
    ),
    (
        "INSTALL_FAILED_UPDATE_INCOMPATIBLE",
        "与已安装应用的签名不一致，需要先卸载再安装",
    ),
    (
        "INSTALL_FAILED_ALREADY_EXISTS",
        "应用已存在，勾选「覆盖安装」后重试",
    ),
    (
        "INSTALL_FAILED_INSUFFICIENT_STORAGE",
        "手表存储空间不足，清理后重试",
    ),
    (
        "INSTALL_FAILED_NO_MATCHING_ABIS",
        "APK 的 CPU 架构与手表不匹配，请使用对应的分包",
    ),
    (
        "INSTALL_FAILED_MISSING_SPLIT",
        "缺少分包：请把 base.apk 与该应用的全部 split APK 一起安装",
    ),
    (
        "INSTALL_FAILED_INVALID_APK",
        "APK 无效：可能缺少主 APK，或文件已损坏",
    ),
    (
        "INSTALL_FAILED_TEST_ONLY",
        "这是测试版 APK，勾选「允许测试包」后重试",
    ),
    ("INSTALL_FAILED_OLDER_SDK", "手表系统版本低于应用要求"),
    (
        "INSTALL_FAILED_USER_RESTRICTED",
        "设备禁止通过 ADB 安装，请在手表上允许「通过 USB 安装」",
    ),
    (
        "INSTALL_PARSE_FAILED_NO_CERTIFICATES",
        "APK 没有有效签名，无法安装",
    ),
    (
        "INSTALL_FAILED_DEXOPT",
        "应用在设备上优化失败，可能是架构或存储问题",
    ),
    (
        "INSTALL_FAILED_CONTAINER_ERROR",
        "无法写入应用目录，通常是存储或权限问题",
    ),
    ("INSTALL_FAILED_ABORTED", "安装被设备取消"),
];

fn describe_failure(text: &str) -> String {
    for (code, hint) in FAILURE_HINTS {
        if text.contains(code) {
            return format!("{hint}（{code}）");
        }
    }
    last_line(text)
        .map(str::to_string)
        .unwrap_or_else(|| "安装失败，设备没有返回原因".to_string())
}

fn last_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).rfind(|line| !line.is_empty())
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apk(name: &str, package: &str, split: Option<&str>) -> ApkFile {
        ApkFile {
            path: PathBuf::from(format!("/tmp/{name}")),
            package: Some(package.to_string()),
            split: split.map(str::to_string),
            label: None,
            version_code: Some(1),
        }
    }

    fn group(package: &str, files: &[&str], splits: usize) -> ApkGroup {
        ApkGroup {
            package: package.to_string(),
            label: None,
            version_code: Some(1),
            base: Some(PathBuf::from(files[0])),
            splits: files[1..splits + 1].iter().map(PathBuf::from).collect(),
        }
    }

    #[test]
    fn downgrade_implies_replace() {
        let options = InstallOptions {
            replace: false,
            downgrade: true,
            ..InstallOptions::default()
        };
        assert_eq!(options.flags(), ["-r", "-d"]);
        // 没有降级时，覆盖安装只带 -r。
        let options = InstallOptions {
            replace: true,
            downgrade: false,
            ..InstallOptions::default()
        };
        assert_eq!(options.flags(), ["-r"]);
        assert_eq!(InstallOptions::default().flags(), ["-r"]);
    }

    #[test]
    fn builds_single_and_split_commands() {
        let options = InstallOptions {
            replace: true,
            grant_permissions: true,
            ..InstallOptions::default()
        };
        let single = install_args(&group("com.foo", &["/tmp/base.apk"], 0), &options);
        assert_eq!(
            single,
            ["install", "-r", "-g", "/tmp/base.apk"]
                .map(str::to_string)
                .to_vec()
        );

        // 分包安装必须用 install-multiple，且主 APK 排在最前。
        let split = install_args(
            &group(
                "com.foo",
                &["/tmp/base.apk", "/tmp/split_a.apk", "/tmp/split_b.apk"],
                2,
            ),
            &options,
        );
        assert_eq!(split[0], "install-multiple");
        assert_eq!(&split[1..3], ["-r", "-g"].map(str::to_string).as_slice());
        assert_eq!(
            &split[3..],
            ["/tmp/base.apk", "/tmp/split_a.apk", "/tmp/split_b.apk"]
                .map(str::to_string)
                .as_slice()
        );
    }

    #[test]
    fn interprets_install_results() {
        let success = AdbOutput {
            code: Some(0),
            stdout: "Performing Streamed Install\nSuccess\n".to_string(),
            stderr: String::new(),
        };
        assert_eq!(
            install_outcome(&success),
            Ok("Success".to_string()),
            "成功时取 adb 的最后一行"
        );

        let downgrade = AdbOutput {
            code: Some(1),
            stdout: String::new(),
            stderr: "adb: failed to install base.apk: Failure [INSTALL_FAILED_VERSION_DOWNGRADE: Downgrade detected]"
                .to_string(),
        };
        let reason = install_outcome(&downgrade).expect_err("downgrade must fail");
        assert!(reason.contains("允许降级"), "{reason}");

        // 未知原因时保留 adb 的最后一行，至少能让用户看到设备原话。
        let unknown = AdbOutput {
            code: Some(1),
            stdout: String::new(),
            stderr: "adb: failed to install x.apk: oops\n".to_string(),
        };
        assert_eq!(
            install_outcome(&unknown),
            Err("adb: failed to install x.apk: oops".to_string())
        );
    }

    #[test]
    fn groups_splits_under_their_package() {
        // 直接用解析结果驱动分组逻辑，避免依赖真实 APK 文件。
        let plan = group_apks(vec![
            apk("split_config.apk", "com.foo", Some("config.arm64_v8a")),
            apk("base.apk", "com.foo", None),
            apk("other.apk", "com.bar", None),
        ]);
        assert_eq!(plan.groups.len(), 2);
        assert_eq!(plan.groups[0].package, "com.foo");
        assert!(plan.groups[0].is_split_install());
        // 主 APK 一定排在最前，即使选择顺序里分包在前。
        assert_eq!(plan.groups[0].files()[0], PathBuf::from("/tmp/base.apk"));
        assert_eq!(plan.groups[0].apk_count(), 2);
        assert_eq!(plan.groups[1].package, "com.bar");
        assert!(!plan.groups[1].is_split_install());
    }

    #[test]
    fn rejects_split_only_and_conflicting_bases() {
        let plan = group_apks(vec![
            apk("split_a.apk", "com.foo", Some("config.arm64_v8a")),
            apk("base_v1.apk", "com.bar", None),
            apk("base_v2.apk", "com.bar", None),
        ]);
        assert!(plan.groups.is_empty());
        assert_eq!(plan.failures.len(), 2);
        assert!(
            plan.failures
                .iter()
                .any(|failure| failure.title == "com.foo" && failure.detail.contains("缺少主 APK"))
        );
        assert!(
            plan.failures
                .iter()
                .any(|failure| failure.title == "com.bar" && failure.detail.contains("多个主 APK"))
        );
    }

    #[test]
    fn reads_package_and_split_from_real_apks() {
        // 由 aapt2 link 生成的最小主 APK 与配置分包。
        let directory = std::env::temp_dir().join("rarebox-install-fixtures");
        std::fs::create_dir_all(&directory).expect("temp dir");
        let base = directory.join("base.apk");
        let split = directory.join("split_config.apk");
        std::fs::write(&base, include_bytes!("testdata/base.apk")).expect("write base");
        std::fs::write(&split, include_bytes!("testdata/split.apk")).expect("write split");

        let base_apk = read_apk(&base).expect("base.apk should parse");
        assert_eq!(
            base_apk.package.as_deref(),
            Some("com.rarebox.splitfixture")
        );
        // 主 APK 没有 split 属性，label 直接写在 manifest 里。
        assert_eq!(base_apk.split, None);
        assert_eq!(base_apk.label.as_deref(), Some("Split Fixture"));
        assert_eq!(base_apk.version_code, Some(42));

        let split_apk = read_apk(&split).expect("split.apk should parse");
        assert_eq!(split_apk.split.as_deref(), Some("config.arm64_v8a"));

        // 两个文件选中后必须归为一组，且走 install-multiple。
        let plan = plan_installs(&[split.clone(), base.clone()]);
        assert!(plan.failures.is_empty());
        assert_eq!(plan.groups.len(), 1);
        let group = &plan.groups[0];
        assert!(group.is_split_install());
        assert_eq!(group.title(), "Split Fixture（com.rarebox.splitfixture）");
        assert_eq!(
            install_args(group, &InstallOptions::default())[0],
            "install-multiple"
        );

        for path in [base, split] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn reports_unreadable_files() {
        let directory = std::env::temp_dir().join("rarebox-install-test");
        std::fs::create_dir_all(&directory).expect("temp dir");
        let path = directory.join("broken.apk");
        std::fs::write(&path, b"not a zip").expect("write fixture");
        let plan = plan_installs(std::slice::from_ref(&path));
        assert!(plan.groups.is_empty());
        assert_eq!(plan.failures.len(), 1);
        assert!(plan.failures[0].detail.contains("不是有效的 APK"));
        let _ = std::fs::remove_file(&path);

        // 重复选择同一个文件只算一次。
        let plan = plan_installs(&[path.clone(), path]);
        assert_eq!(plan.failures.len(), 1);
    }
}
