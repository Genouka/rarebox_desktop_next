//! APK 元数据解析：直接读取 `AndroidManifest.xml`（AXML）与 `resources.arsc`（ARSC）。
//!
//! 设备端只需要用 `unzip -p` 取出这两个文件，解析全部在本地完成，
//! 不依赖 aapt/aapt2，也不需要把 APK 整包拉回电脑。
//!
//! 解析目标是应用名称（label）、版本名与版本号：label 可能直接是字符串，
//! 也可能是指向 `resources.arsc` 的资源 ID，需要在资源表的字符串池中查表。

/// AXML 与 ARSC 共用的 chunk 类型。
const CHUNK_STRING_POOL: u16 = 0x0001;
const CHUNK_XML_RESOURCE_MAP: u16 = 0x0180;
const CHUNK_XML_START_TAG: u16 = 0x0102;
const CHUNK_XML_END_TAG: u16 = 0x0103;
const CHUNK_TABLE_PACKAGE: u16 = 0x0200;
const CHUNK_TABLE_TYPE: u16 = 0x0201;

/// `android:` 命名空间中的属性 ID。
const ATTR_LABEL: u32 = 0x0101_0001;
const ATTR_NAME: u32 = 0x0101_0003;
const ATTR_VERSION_CODE: u32 = 0x0101_021b;
const ATTR_VERSION_NAME: u32 = 0x0101_021c;

/// Res_value 数据类型。
const TYPE_REFERENCE: u8 = 0x01;
const TYPE_STRING: u8 = 0x03;
const TYPE_INT_DEC: u8 = 0x10;
const TYPE_INT_HEX: u8 = 0x11;

/// ResTable_type 的偏移表变体。
const FLAG_SPARSE: u8 = 0x01;
const FLAG_OFFSET16: u8 = 0x02;

/// ResTable_entry 的标志位。
const ENTRY_FLAG_COMPLEX: u16 = 0x0001;
const ENTRY_FLAG_COMPACT: u16 = 0x0008;

const LAUNCHER_CATEGORY: &str = "android.intent.category.LAUNCHER";

/// `<manifest>` 上的无命名空间属性名。它们没有 `android:` 资源 ID，只能按名字匹配。
const ATTR_NAME_PACKAGE: &str = "package";
const ATTR_NAME_SPLIT: &str = "split";

/// 从一个 APK 的 manifest 与资源表里解析出的信息。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApkMetadata {
    /// 应用名称；解析失败时为 `None`，由调用方退回包名。
    pub label: Option<String>,
    /// `versionName`，例如 `3.5`。
    pub version_name: Option<String>,
    /// `versionCode`。
    pub version_code: Option<u64>,
    /// manifest 根节点的 `package` 属性。
    pub package: Option<String>,
    /// `split` 属性；主 APK 没有这个属性，分包才有。
    pub split: Option<String>,
}

/// 解析 manifest 与（可选的）资源表。
///
/// 两个参数都允许为空：取不到 manifest 时返回默认值，
/// 取不到资源表时只能解析出直接写在 manifest 里的 label。
pub fn metadata(manifest: Option<&[u8]>, resources: Option<&[u8]>) -> ApkMetadata {
    let Some(manifest) = manifest.filter(|bytes| bytes.len() >= 8) else {
        return ApkMetadata::default();
    };
    let parsed = parse_manifest(manifest);
    let label = match parsed.label {
        Some(Label::Literal(text)) => Some(text),
        Some(Label::Resource(id)) => resources.and_then(|bytes| resolve_string(bytes, id)),
        None => None,
    };
    ApkMetadata {
        label: label.filter(|label| !label.trim().is_empty()),
        version_name: parsed.version_name,
        version_code: parsed.version_code,
        package: parsed.package,
        split: parsed.split,
    }
}

#[derive(Clone, Debug)]
enum Label {
    Literal(String),
    Resource(u32),
}

#[derive(Default)]
struct ManifestInfo {
    label: Option<Label>,
    version_name: Option<String>,
    version_code: Option<u64>,
    package: Option<String>,
    split: Option<String>,
}

/// 遍历 AXML 的标签，取 `application` 的 label；没有时退回启动 Activity 的 label。
fn parse_manifest(data: &[u8]) -> ManifestInfo {
    let mut info = ManifestInfo::default();
    let Some(root_header) = read_u16(data, 2) else {
        return info;
    };
    let mut pool: Option<StringPool<'_>> = None;
    let mut resource_ids: &[u8] = &[];
    let mut activity_label: Option<Label> = None;
    let mut launcher_label: Option<Label> = None;
    let mut offset = root_header as usize;

    while let Some((kind, header_size, size)) = chunk_at(data, offset) {
        match kind {
            CHUNK_STRING_POOL => pool = StringPool::parse(data, offset),
            CHUNK_XML_RESOURCE_MAP => {
                resource_ids = data
                    .get(offset + header_size..offset + size)
                    .unwrap_or_default();
            }
            CHUNK_XML_START_TAG => {
                let Some(pool) = pool.as_ref() else {
                    offset += size;
                    continue;
                };
                let Some(tag) = tag_name(data, offset, header_size, pool) else {
                    offset += size;
                    continue;
                };
                match tag.as_str() {
                    "manifest" => {
                        for attr in attributes(data, offset, header_size, resource_ids) {
                            match attr.id {
                                ATTR_VERSION_CODE => {
                                    info.version_code = attr.integer();
                                }
                                ATTR_VERSION_NAME => {
                                    info.version_name = attr.string(pool);
                                }
                                _ => match attr.name(pool).as_deref() {
                                    Some(ATTR_NAME_PACKAGE) => info.package = attr.string(pool),
                                    Some(ATTR_NAME_SPLIT) => info.split = attr.string(pool),
                                    _ => {}
                                },
                            }
                        }
                    }
                    "application" => {
                        for attr in attributes(data, offset, header_size, resource_ids) {
                            if attr.id == ATTR_LABEL {
                                info.label = attr.label(pool);
                            }
                        }
                    }
                    "activity" | "activity-alias" => {
                        activity_label = None;
                        for attr in attributes(data, offset, header_size, resource_ids) {
                            if attr.id == ATTR_LABEL {
                                activity_label = attr.label(pool);
                            }
                        }
                    }
                    "category" => {
                        let is_launcher = attributes(data, offset, header_size, resource_ids)
                            .into_iter()
                            .any(|attr| {
                                attr.id == ATTR_NAME
                                    && attr.string(pool).as_deref() == Some(LAUNCHER_CATEGORY)
                            });
                        if is_launcher && launcher_label.is_none() {
                            launcher_label = activity_label.clone();
                        }
                    }
                    _ => {}
                }
            }
            CHUNK_XML_END_TAG => {
                if let Some(pool) = pool.as_ref()
                    && tag_name(data, offset, header_size, pool).as_deref() == Some("activity")
                {
                    activity_label = None;
                }
            }
            _ => {}
        }
        offset += size;
    }

    if info.label.is_none() {
        info.label = launcher_label;
    }
    info
}

struct Attribute {
    /// `android:` 属性的资源 ID；无命名空间的属性（如 `package`、`split`）为 0。
    id: u32,
    /// 属性名在字符串池中的下标，用于识别无资源 ID 的属性。
    name_index: u32,
    data_type: u8,
    data: u32,
}

impl Attribute {
    fn string(&self, pool: &StringPool<'_>) -> Option<String> {
        (self.data_type == TYPE_STRING)
            .then(|| pool.get(self.data))
            .flatten()
    }

    /// 属性名，用于没有资源 ID 的属性。
    fn name(&self, pool: &StringPool<'_>) -> Option<String> {
        pool.get(self.name_index)
    }

    fn integer(&self) -> Option<u64> {
        matches!(self.data_type, TYPE_INT_DEC | TYPE_INT_HEX).then_some(self.data as u64)
    }

    fn label(&self, pool: &StringPool<'_>) -> Option<Label> {
        match self.data_type {
            TYPE_STRING => self.string(pool).map(Label::Literal),
            TYPE_REFERENCE => Some(Label::Resource(self.data)),
            _ => None,
        }
    }
}

fn tag_name(
    data: &[u8],
    chunk: usize,
    header_size: usize,
    pool: &StringPool<'_>,
) -> Option<String> {
    let index = read_u32(data, chunk + header_size + 4)?;
    pool.get(index)
}

/// 读取一个开始标签的全部属性。
fn attributes<'a>(
    data: &'a [u8],
    chunk: usize,
    header_size: usize,
    resource_ids: &'a [u8],
) -> Vec<Attribute> {
    let ext = chunk + header_size;
    let Some(start) = read_u16(data, ext + 8) else {
        return Vec::new();
    };
    let Some(stride) = read_u16(data, ext + 10) else {
        return Vec::new();
    };
    let Some(count) = read_u16(data, ext + 12) else {
        return Vec::new();
    };
    let mut attributes = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        let at = ext + start as usize + index * stride as usize;
        let Some(name_index) = read_u32(data, at + 4) else {
            break;
        };
        let Some(data_type) = data.get(at + 15).copied() else {
            break;
        };
        let Some(data) = read_u32(data, at + 16) else {
            break;
        };
        attributes.push(Attribute {
            id: read_u32(resource_ids, name_index as usize * 4).unwrap_or(0),
            name_index,
            data_type,
            data,
        });
    }
    attributes
}

/// 在资源表里查一个资源 ID，沿着引用链取到字符串。
fn resolve_string(resources: &[u8], id: u32) -> Option<String> {
    let mut current = id;
    for _ in 0..8 {
        let (data_type, data) = find_entry(resources, current)?;
        match data_type {
            TYPE_STRING => return global_string(resources, data),
            TYPE_REFERENCE => current = data,
            _ => return None,
        }
    }
    None
}

/// 资源表根节点的字符串池。
fn global_string(resources: &[u8], index: u32) -> Option<String> {
    let header = read_u16(resources, 2)? as usize;
    StringPool::parse(resources, header)?.get(index)
}

fn find_entry(resources: &[u8], id: u32) -> Option<(u8, u32)> {
    let package_id = (id >> 24) as u8;
    let type_id = (id >> 16) as u8 as u16;
    let entry_id = id & 0xffff;
    let table_header = read_u16(resources, 2)? as usize;
    // ResTable_header.size 是从文件开头算起的整块长度。
    let size = read_u32(resources, 4)? as usize;
    let end = size.min(resources.len());
    if table_header > end {
        return None;
    }

    let mut offset = table_header;
    while let Some((kind, header_size, size)) = chunk_at(resources, offset) {
        if offset + size > end {
            break;
        }
        if kind == CHUNK_TABLE_PACKAGE
            && read_u32(resources, offset + 8).map(|value| value as u8) == Some(package_id)
            && let Some(value) =
                find_in_package(resources, offset, header_size, size, type_id, entry_id)
        {
            return Some(value);
        }
        offset += size;
    }
    None
}

/// 在包内按 type/entry 找值，优先选择默认配置（不带语言、密度等限定符）。
fn find_in_package(
    resources: &[u8],
    package: usize,
    header_size: usize,
    size: usize,
    type_id: u16,
    entry_id: u32,
) -> Option<(u8, u32)> {
    let end = package + size;
    let mut offset = package + header_size;
    let mut fallback = None;
    while offset + 8 <= end {
        let Some((kind, chunk_header, chunk_size)) = chunk_at(resources, offset) else {
            break;
        };
        if offset + chunk_size > end {
            break;
        }
        if kind == CHUNK_TABLE_TYPE
            && resources.get(offset + 8).map(|id| *id as u16) == Some(type_id)
            && let Some(value) = read_entry(resources, offset, chunk_header, entry_id)
        {
            if is_default_config(resources, offset, chunk_header) {
                return Some(value);
            }
            fallback = fallback.or(Some(value));
        }
        offset += chunk_size;
    }
    fallback
}

fn is_default_config(resources: &[u8], chunk: usize, header_size: usize) -> bool {
    let Some(config_size) = read_u32(resources, chunk + 20) else {
        return false;
    };
    let start = chunk + 24;
    let end = chunk + 20 + (config_size as usize).clamp(4, header_size.saturating_sub(20));
    match resources.get(start..end) {
        Some(rest) => rest.iter().all(|byte| *byte == 0),
        None => false,
    }
}

fn read_entry(
    resources: &[u8],
    chunk: usize,
    header_size: usize,
    entry_id: u32,
) -> Option<(u8, u32)> {
    let flags = *resources.get(chunk + 9)?;
    let entry_count = read_u32(resources, chunk + 12)?;
    let entries_start = read_u32(resources, chunk + 16)? as usize;

    let entry_offset = if flags & FLAG_SPARSE != 0 {
        // 稀疏表：有序的 (索引, 偏移/4) 对。
        let mut found = None;
        for index in 0..entry_count as usize {
            let at = chunk + header_size + index * 4;
            let candidate = read_u16(resources, at)?;
            if candidate as u32 == entry_id {
                found = Some(read_u16(resources, at + 2)? as usize * 4);
                break;
            }
            if candidate as u32 > entry_id {
                break;
            }
        }
        found?
    } else if flags & FLAG_OFFSET16 != 0 {
        if entry_id >= entry_count {
            return None;
        }
        let at = chunk + header_size + entry_id as usize * 2;
        match read_u16(resources, at)? {
            0xffff => return None,
            offset => offset as usize * 4,
        }
    } else {
        if entry_id >= entry_count {
            return None;
        }
        let at = chunk + header_size + entry_id as usize * 4;
        match read_u32(resources, at)? {
            u32::MAX => return None,
            offset => offset as usize,
        }
    };

    let entry = chunk + entries_start + entry_offset;
    let entry_size = read_u16(resources, entry)?;
    let entry_flags = read_u16(resources, entry + 2)?;
    if entry_flags & ENTRY_FLAG_COMPLEX != 0 {
        return None;
    }
    if entry_flags & ENTRY_FLAG_COMPACT != 0 {
        return Some(((entry_flags >> 8) as u8, read_u32(resources, entry + 4)?));
    }
    let value = entry + entry_size as usize;
    Some((*resources.get(value + 3)?, read_u32(resources, value + 4)?))
}

/// 读取 chunk 头：类型、头部长度、整块长度。
fn chunk_at(data: &[u8], offset: usize) -> Option<(u16, usize, usize)> {
    let kind = read_u16(data, offset)?;
    let header_size = read_u16(data, offset + 2)? as usize;
    let size = read_u32(data, offset + 4)? as usize;
    if size < 8 || header_size < 8 || header_size > size || offset.checked_add(size)? > data.len() {
        return None;
    }
    Some((kind, header_size, size))
}

fn read_u16(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// 资源文件里的字符串池。AXML 与 ARSC 的结构一致，只是编码（UTF-8/UTF-16）可能不同。
struct StringPool<'a> {
    data: &'a [u8],
    offsets: &'a [u8],
    base: usize,
    count: u32,
    utf8: bool,
}

impl<'a> StringPool<'a> {
    fn parse(data: &'a [u8], chunk: usize) -> Option<Self> {
        if read_u16(data, chunk)? != CHUNK_STRING_POOL {
            return None;
        }
        let header_size = read_u16(data, chunk + 2)? as usize;
        let count = read_u32(data, chunk + 8)?;
        let flags = read_u32(data, chunk + 16)?;
        let strings_start = read_u32(data, chunk + 20)? as usize;
        Some(Self {
            data,
            offsets: data.get(chunk + header_size..)?,
            base: chunk.checked_add(strings_start)?,
            count,
            utf8: flags & 0x100 != 0,
        })
    }

    fn get(&self, index: u32) -> Option<String> {
        if index >= self.count {
            return None;
        }
        let offset = read_u32(self.offsets, index as usize * 4)? as usize;
        let mut at = self.base.checked_add(offset)?;
        if self.utf8 {
            // 先是 UTF-16 长度（用不到），再是 UTF-8 字节长度，两者都是 1~2 字节变长编码。
            let mut first = *self.data.get(at)?;
            at += 1;
            if first & 0x80 != 0 {
                at += 1;
            }
            first = *self.data.get(at)?;
            at += 1;
            let length = if first & 0x80 != 0 {
                at += 1;
                (((first & 0x7f) as usize) << 8) | *self.data.get(at - 1)? as usize
            } else {
                first as usize
            };
            let bytes = self.data.get(at..at + length)?;
            Some(String::from_utf8_lossy(bytes).into_owned())
        } else {
            let mut length = read_u16(self.data, at)? as usize;
            at += 2;
            if length & 0x8000 != 0 {
                length = ((length & 0x7fff) << 16) | read_u16(self.data, at)? as usize;
                at += 2;
            }
            let bytes = self.data.get(at..at + length * 2)?;
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|unit| u16::from_le_bytes(*unit))
                .collect();
            Some(String::from_utf16_lossy(&units))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 由 aapt2 编译出的测试 APK 中取出的 `AndroidManifest.xml` 与 `resources.arsc`。
    const LABELED_MANIFEST: &[u8] = include_bytes!("testdata/labeled-manifest.bin");
    const LAUNCHER_MANIFEST: &[u8] = include_bytes!("testdata/launcher-manifest.bin");
    const RESOURCES: &[u8] = include_bytes!("testdata/resources.arsc");

    #[test]
    fn reads_label_from_resource_table() {
        let metadata = metadata(Some(LABELED_MANIFEST), Some(RESOURCES));
        assert_eq!(metadata.label.as_deref(), Some("Fixture Watch App"));
        assert_eq!(metadata.version_name.as_deref(), Some("1.2.3"));
        assert_eq!(metadata.version_code, Some(42));
    }

    #[test]
    fn reads_package_and_split_attributes() {
        // manifest 根节点的 `package` 没有 android: 资源 ID，只能按属性名识别。
        let metadata = metadata(Some(LABELED_MANIFEST), None);
        assert_eq!(metadata.package.as_deref(), Some("com.rarebox.fixture"));
        // 主 APK 没有 split 属性。
        assert_eq!(metadata.split, None);
    }

    #[test]
    fn falls_back_to_launcher_activity_label() {
        let metadata = metadata(Some(LAUNCHER_MANIFEST), Some(RESOURCES));
        assert_eq!(metadata.label.as_deref(), Some("Watch Face"));
        assert_eq!(metadata.version_code, Some(7));
        assert_eq!(metadata.version_name, None);
    }

    #[test]
    fn tolerates_missing_inputs() {
        assert_eq!(metadata(None, None), ApkMetadata::default());
        assert_eq!(metadata(Some(b"not an apk"), None), ApkMetadata::default());
        // 缺少资源表时仍能读到版本信息，label 保持未知。
        let metadata = metadata(Some(LABELED_MANIFEST), None);
        assert_eq!(metadata.label, None);
        assert_eq!(metadata.version_name.as_deref(), Some("1.2.3"));
        assert_eq!(metadata.version_code, Some(42));
    }
}
