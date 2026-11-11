use crate::{model::*, platform};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn connect(path: &Path) -> Result<Connection> {
    let db = Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(3))?;
    db.pragma_update(None, "foreign_keys", "ON")?;
    Ok(db)
}
pub fn initialize(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("database_unavailable", "无效索引目录"))?;
    std::fs::create_dir_all(parent)?;
    let db = connect(path)?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > 1 {
        return Err(AppError::new(
            "database_unavailable",
            "索引来自更新版本的应用，无法读取。",
        ));
    }
    db.execute_batch(include_str!("schema.sql"))?;
    db.execute("UPDATE scans SET state='interrupted',reason='应用上次未正常结束',ended=?1,has_result=0 WHERE state IN ('preparing','scanning','cancelling','finalizing')",[now()])?;
    Ok(())
}
pub fn scan(db: &Connection, id: &str) -> Result<Scan> {
    db.query_row("SELECT id,scope_key,path_display,state,reason,started,ended,entries,files,errors,excluded,has_result,current_path FROM scans WHERE id=?1",[id],read_scan).optional()?.ok_or_else(||AppError::new("result_expired","扫描记录不存在或已过期。"))
}
fn read_scan(r: &rusqlite::Row) -> rusqlite::Result<Scan> {
    let started: i64 = r.get(5)?;
    let ended: Option<i64> = r.get(6)?;
    Ok(Scan {
        id: r.get(0)?,
        scope_key: r.get(1)?,
        path_display: r.get(2)?,
        state: r.get(3)?,
        reason: r.get(4)?,
        started_at: started,
        ended_at: ended,
        entries: r.get::<_, i64>(7)?.to_string(),
        files: r.get::<_, i64>(8)?.to_string(),
        errors: r.get::<_, i64>(9)?.to_string(),
        excluded: r.get::<_, i64>(10)?.to_string(),
        has_result: r.get(11)?,
        revision: "1".into(),
        root_id: "1".into(),
        current_path: r.get(12)?,
        elapsed_ms: ended.unwrap_or_else(now).saturating_sub(started),
    })
}
pub fn scans(db: &Connection) -> Result<Vec<Scan>> {
    let mut q=db.prepare("SELECT id,scope_key,path_display,state,reason,started,ended,entries,files,errors,excluded,has_result,current_path FROM scans ORDER BY started DESC,id DESC LIMIT 2")?;
    let rows = q
        .query_map([], read_scan)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}
pub fn display_id(db: &Connection) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT value FROM app_state WHERE key='display_scan_id'",
            [],
            |r| r.get(0),
        )
        .optional()?)
}
pub fn require_result(db: &Connection, scan_id: &str) -> Result<Scan> {
    let s = scan(db, scan_id)?;
    if !s.has_result {
        return Err(AppError::new("result_not_ready", "结果尚未完成汇总。"));
    }
    Ok(s)
}
pub fn show(db: &Connection, id: &str) -> Result<()> {
    require_result(db, id)?;
    db.execute("INSERT INTO app_state(key,value) VALUES('display_scan_id',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[id])?;
    Ok(())
}
pub fn settings(db: &Connection) -> Result<Settings> {
    let value: Option<String> = db
        .query_row(
            "SELECT value FROM app_state WHERE key='settings'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match value {
        None => Ok(Settings::default()),
        Some(s) => serde_json::from_str(&s)
            .map_err(|_| AppError::new("database_unavailable", "偏好设置损坏。")),
    }
}
pub fn save_settings(db: &mut Connection, new: &Settings) -> Result<Settings> {
    if !["binary", "decimal"].contains(&new.unit.as_str()) {
        return Err(AppError::new("invalid_argument", "不支持的容量单位。"));
    }
    let tx = db.transaction()?;
    let previous = settings(&tx)?;
    if new.version != previous.version {
        return Err(AppError::new("settings_conflict", "设置已更新，请重试。"));
    }
    let mut value = new.clone();
    value.version = value
        .version
        .checked_add(1)
        .ok_or_else(|| AppError::new("settings_conflict", "版本号溢出"))?;
    tx.execute("INSERT INTO app_state(key,value) VALUES('settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(&value).unwrap()])?;
    tx.commit()?;
    Ok(value)
}
pub fn restore_scopes(db: &Connection) -> Result<Vec<Scope>> {
    let mut q = db.prepare(
        "SELECT scope_key,root,path_display,device,inode FROM scans ORDER BY started DESC LIMIT 2",
    )?;
    let mut result = Vec::new();
    let rows = q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Vec<u8>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (key, raw, display, dev, ino) = row?;
        if result.iter().any(|s: &Scope| s.key == key) {
            continue;
        }
        let mut s = Scope {
            id: uuid::Uuid::new_v4().to_string(),
            key,
            path_display: display,
            path: platform::from_bytes(raw),
            device: dev.parse().unwrap_or(0),
            inode: ino.parse().unwrap_or(0),
            available: false,
        };
        s.available = platform::check_scope(&s).is_ok();
        result.push(s);
    }
    Ok(result)
}
pub const ENTRY_COLUMNS:&str="id,parent,name,path_display,kind,logical,allocated,known_l,known_a,unknown_l,unknown_a,gaps,observed_l,observed_a,files,canonical,excluded,note,modified";
pub fn read_entry(r: &rusqlite::Row) -> rusqlite::Result<Entry> {
    let num = |i| r.get::<_, i64>(i).map(|n| n.to_string());
    let opt = |i| r.get::<_, Option<i64>>(i).map(|v| v.map(|n| n.to_string()));
    Ok(Entry {
        id: num(0)?,
        parent_id: opt(1)?,
        name: r.get(2)?,
        path_display: r.get(3)?,
        kind: r.get(4)?,
        logical: opt(5)?,
        allocated: opt(6)?,
        known_logical: num(7)?,
        known_allocated: num(8)?,
        unknown_logical: num(9)?,
        unknown_allocated: num(10)?,
        gaps: num(11)?,
        observed_logical: num(12)?,
        observed_allocated: num(13)?,
        files: num(14)?,
        canonical_id: opt(15)?,
        excluded: r.get(16)?,
        note: r.get(17)?,
        modified_at: r.get(18)?,
    })
}
pub fn entry(db: &Connection, sid: &str, eid: i64) -> Result<Entry> {
    db.query_row(
        &format!("SELECT {ENTRY_COLUMNS} FROM entries WHERE scan=?1 AND id=?2"),
        params![sid, eid],
        read_entry,
    )
    .optional()?
    .ok_or_else(|| AppError::new("entry_missing", "扫描中没有这个项目。"))
}
pub fn native_path(
    db: &Connection,
    sid: &str,
    eid: &str,
) -> Result<(PathBuf, Vec<u8>, Option<(u64, u64)>)> {
    require_result(db, sid)?;
    let root: Vec<u8> = db.query_row("SELECT root FROM scans WHERE id=?1", [sid], |r| r.get(0))?;
    let (raw, dev, ino): (Vec<u8>, Option<String>, Option<String>) = db
        .query_row(
            "SELECT path,device,inode FROM entries WHERE scan=?1 AND id=?2",
            params![sid, id(eid)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::new("entry_missing", "项目不存在。"))?;
    let identity = dev
        .and_then(|d| d.parse().ok())
        .zip(ino.and_then(|i| i.parse().ok()));
    Ok((platform::from_bytes(root), raw, identity))
}
