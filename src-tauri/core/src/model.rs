use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, AppError>;
impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AppError {}
impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        let code = match e.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DiskFull) => "database_full",
            _ => "database_unavailable",
        };
        Self::new(code, "本地索引操作未完成，请检查磁盘空间和应用目录权限。")
    }
}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::new(
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                "permission_denied"
            } else {
                "filesystem_error"
            },
            e.to_string(),
        )
    }
}
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
pub fn id(s: &str) -> Result<i64> {
    s.parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| AppError::new("invalid_argument", "无效条目 ID"))
}

#[derive(Debug, Clone, Serialize)]
pub struct Scope {
    pub id: String,
    pub key: String,
    pub path_display: String,
    pub available: bool,
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(skip)]
    pub device: u64,
    #[serde(skip)]
    pub inode: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Volume {
    pub id: String,
    pub name: String,
    pub path_display: String,
    pub total: String,
    pub free: String,
    pub available: String,
    pub used: String,
    pub sampled_at: i64,
    pub is_home: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct Scan {
    pub id: String,
    pub scope_key: String,
    pub path_display: String,
    pub state: String,
    pub reason: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub entries: String,
    pub files: String,
    pub errors: String,
    pub excluded: String,
    pub has_result: bool,
    pub revision: String,
    pub root_id: String,
    pub current_path: String,
    pub elapsed_ms: i64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub scan: Scan,
    pub sequence: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub path_display: String,
    pub kind: String,
    pub logical: Option<String>,
    pub allocated: Option<String>,
    pub known_logical: String,
    pub known_allocated: String,
    pub unknown_logical: String,
    pub unknown_allocated: String,
    pub gaps: String,
    pub observed_logical: String,
    pub observed_allocated: String,
    pub files: String,
    pub canonical_id: Option<String>,
    pub excluded: bool,
    pub note: Option<String>,
    pub modified_at: Option<i64>,
}
impl Entry {
    pub fn value(&self, metric: Metric) -> i64 {
        match metric {
            Metric::Allocated => &self.known_allocated,
            Metric::Logical => &self.known_logical,
        }
        .parse()
        .unwrap_or(0)
    }
    pub fn unknown(&self, metric: Metric) -> bool {
        let (unknown, observed) = match metric {
            Metric::Allocated => (&self.unknown_allocated, &self.observed_allocated),
            Metric::Logical => (&self.unknown_logical, &self.observed_logical),
        };
        unknown != "0" && observed == "0"
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    Allocated,
    Logical,
}
impl Default for Metric {
    fn default() -> Self {
        Self::Allocated
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Query {
    pub scan_id: String,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub metric: Metric,
    #[serde(default)]
    pub search: String,
    #[serde(default = "yes")]
    pub descending: bool,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "page_size")]
    pub limit: u32,
    #[serde(default)]
    pub group_token: Option<String>,
}
fn yes() -> bool {
    true
}
fn page_size() -> u32 {
    100
}
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub scan_id: String,
    pub revision: String,
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct DirectoryMap {
    pub scan_id: String,
    pub revision: String,
    pub items: Vec<Entry>,
    pub other_count: String,
    pub other_bytes: String,
    pub group_token: Option<String>,
    pub total: String,
    pub unknown_count: String,
    pub zero_count: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct ScanError {
    pub id: String,
    pub entry_id: Option<String>,
    pub path_display: String,
    pub code: String,
    pub message: String,
    pub occurred_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub unit: String,
    pub metric: Metric,
    pub version: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            unit: "binary".into(),
            metric: Metric::Allocated,
            version: 1,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Detail {
    pub entry: Entry,
    pub ancestors: Vec<Entry>,
    pub canonical: Option<Entry>,
    pub description: String,
}

pub fn description(entry: &Entry) -> String {
    if entry
        .canonical_id
        .as_ref()
        .is_some_and(|id| id != &entry.id)
    {
        return "这是同一文件的另一条硬链接路径。文件原始大小仍然有效，本位置的计量贡献已归入另一条路径。".into();
    }
    if entry.name == "Docker.raw" {
        return "Docker 稀疏虚拟磁盘。逻辑大小可能远大于分配量，不等于删除后能够释放的空间。请通过 Docker 管理镜像、容器及数据卷。".into();
    }
    if entry.path_display.contains("/Caches/") || entry.name == "Caches" {
        return "根据所在路径判断为缓存相关区域；其中仍可能包含本地历史和恢复数据，不代表可以整体删除。".into();
    }
    if entry.kind == "directory" {
        return "按本次扫描可访问文件的归属贡献汇总。目录总量包含子项，不能与子项重复相加；APFS 共享块与快照可能影响实际空间。".into();
    }
    "文件系统报告的分配大小不等于独占物理空间，也不等于删除后可释放空间。本工具不读取文件正文。"
        .into()
}
