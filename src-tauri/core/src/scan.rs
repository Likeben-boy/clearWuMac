use crate::{
    model::*,
    platform::{self, Directory},
    store,
};
use rusqlite::{params, OptionalExtension};
use std::{
    ffi::OsString,
    os::unix::ffi::OsStringExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, SyncSender},
        Arc,
    },
    time::{Duration, Instant},
};

struct RawEntry {
    id: i64,
    parent: Option<i64>,
    depth: i64,
    path: PathBuf,
    kind: String,
    device: Option<String>,
    inode: Option<String>,
    logical: Option<i64>,
    allocated: Option<i64>,
    modified: Option<i64>,
    excluded: bool,
    note: Option<String>,
}
enum Event {
    Entry(RawEntry),
    Closed(i64, bool),
    Error(Option<i64>, String, AppError),
    End(Option<String>),
}
struct Batcher {
    sender: SyncSender<Vec<Event>>,
    batch: Vec<Event>,
    bytes: usize,
    last: Instant,
}
impl Batcher {
    fn push(&mut self, e: Event) -> bool {
        self.bytes += match &e {
            Event::Entry(r) => platform::path_bytes(&r.path).len() * 3 + 256,
            _ => 256,
        };
        self.batch.push(e);
        if self.batch.len() >= 500
            || self.bytes >= 2 * 1024 * 1024
            || self.last.elapsed() >= Duration::from_millis(250)
        {
            return self.flush();
        }
        true
    }
    fn flush(&mut self) -> bool {
        if self.batch.is_empty() {
            return true;
        }
        self.bytes = 0;
        self.last = Instant::now();
        self.sender.send(std::mem::take(&mut self.batch)).is_ok()
    }
}
fn raw(
    id: i64,
    parent: Option<i64>,
    depth: i64,
    path: PathBuf,
    s: Option<&libc::stat>,
) -> RawEntry {
    let kind = s
        .map(|s| match s.st_mode as u32 & libc::S_IFMT as u32 {
            m if m == libc::S_IFDIR as u32 => "directory",
            m if m == libc::S_IFREG as u32 => "file",
            m if m == libc::S_IFLNK as u32 => "symlink",
            _ => "special",
        })
        .unwrap_or("unknown");
    let logical = if kind == "file" {
        s.and_then(|s| i64::try_from(s.st_size).ok())
            .filter(|n| *n >= 0)
    } else {
        None
    };
    let allocated = if kind == "file" {
        s.and_then(|s| i64::try_from(s.st_blocks).ok())
            .filter(|n| *n >= 0)
            .and_then(|n| n.checked_mul(512))
    } else {
        None
    };
    RawEntry {
        id,
        parent,
        depth,
        path,
        kind: kind.into(),
        device: s.map(|s| (s.st_dev as u64).to_string()),
        inode: s.map(|s| (s.st_ino as u64).to_string()),
        logical,
        allocated,
        modified: s.and_then(|s| s.st_mtime.checked_mul(1000)),
        excluded: matches!(kind, "symlink" | "special"),
        note: match kind {
            "symlink" => Some("符号链接：按策略不跟随，不计入普通文件累计。".into()),
            "special" => Some("特殊文件：按策略不读取、不计入。".into()),
            _ => None,
        },
    }
}
struct Walker {
    out: Batcher,
    cancel: Arc<AtomicBool>,
    scope: Scope,
    exclude: Vec<PathBuf>,
    next: i64,
    fatal: Option<String>,
}
impl Walker {
    fn visit(&mut self, mut dir: Directory, id: i64, path: &std::path::Path, depth: i64) -> bool {
        let mut incomplete = false;
        loop {
            if self.cancel.load(Ordering::Relaxed) || self.fatal.is_some() {
                incomplete = true;
                break;
            }
            let name = match dir.next_name() {
                Ok(Some(n)) => n,
                Ok(None) => break,
                Err(e) => {
                    self.out
                        .push(Event::Error(Some(id), path.to_string_lossy().into(), e));
                    incomplete = true;
                    break;
                }
            };
            self.next += 1;
            let child_id = self.next;
            let child_path = path.join(OsString::from_vec(name.as_bytes().to_vec()));
            if self.next % 500 == 0 && platform::check_scope(&self.scope).is_err() {
                self.fatal = Some("扫描根目录已失效或磁盘断开。".into());
                incomplete = true;
                break;
            }
            let stat = match dir.child_metadata(&name) {
                Ok(s) => s,
                Err(e) => {
                    if !self.out.push(Event::Entry(raw(
                        child_id,
                        Some(id),
                        depth + 1,
                        child_path.clone(),
                        None,
                    ))) {
                        return false;
                    }
                    if !self.out.push(Event::Error(
                        Some(child_id),
                        child_path.to_string_lossy().into(),
                        e,
                    )) {
                        return false;
                    }
                    continue;
                }
            };
            let mut item = raw(
                child_id,
                Some(id),
                depth + 1,
                child_path.clone(),
                Some(&stat),
            );
            if self.exclude.iter().any(|p| child_path.starts_with(p)) {
                item.excluded = true;
                item.note = Some("应用自身数据目录：按策略排除。".into());
            }
            if stat.st_dev as u64 != self.scope.device {
                item.excluded = true;
                item.note = Some("其他挂载卷：按策略不跨卷。".into());
            }
            let descend = item.kind == "directory" && !item.excluded;
            let overflow =
                item.kind == "file" && (item.logical.is_none() || item.allocated.is_none());
            if !self.out.push(Event::Entry(item)) {
                return false;
            }
            if overflow
                && !self.out.push(Event::Error(
                    Some(child_id),
                    child_path.to_string_lossy().into(),
                    AppError::new("size_overflow", "文件大小未知或超出索引整数范围。"),
                ))
            {
                return false;
            }
            if descend {
                let opened = if depth >= 128 {
                    Err(AppError::new(
                        "depth_limit",
                        "目录过深，已停止深入以限制系统资源使用。",
                    ))
                } else {
                    dir.child(&name, &stat)
                };
                match opened {
                    Ok(child) => {
                        if !self.visit(child, child_id, &child_path, depth + 1) {
                            return false;
                        }
                    }
                    Err(e) => {
                        if !self.out.push(Event::Error(
                            Some(child_id),
                            child_path.to_string_lossy().into(),
                            e,
                        )) {
                            return false;
                        }
                        if !self.out.push(Event::Closed(child_id, true)) {
                            return false;
                        }
                    }
                }
            }
        }
        self.out.push(Event::Closed(id, incomplete))
    }
    fn run(mut self) {
        match Directory::root(&self.scope) {
            Ok(root) => match root.metadata() {
                Ok(s) => {
                    let path = self.scope.path.clone();
                    if self
                        .out
                        .push(Event::Entry(raw(1, None, 0, path.clone(), Some(&s))))
                    {
                        self.visit(root, 1, &path, 0);
                    }
                }
                Err(e) => {
                    self.fatal = Some(e.message.clone());
                    self.out
                        .push(Event::Error(None, self.scope.path_display.clone(), e));
                }
            },
            Err(e) => {
                self.fatal = Some(e.message.clone());
                self.out
                    .push(Event::Error(None, self.scope.path_display.clone(), e));
            }
        }
        if self.fatal.is_none() && platform::check_scope(&self.scope).is_err() {
            self.fatal = Some("扫描根目录已失效或磁盘断开。".into());
        }
        self.out.push(Event::End(self.fatal.clone()));
        self.out.flush();
    }
}

pub fn run(
    db_path: &std::path::Path,
    sid: &str,
    scope: Scope,
    exclude: Vec<PathBuf>,
    cancel: Arc<AtomicBool>,
    emit: &dyn Fn(Progress),
) -> Result<()> {
    let mut db = store::connect(db_path)?;
    db.execute("UPDATE scans SET state='scanning' WHERE id=?1", [sid])?;
    let (sender, receiver) = sync_channel(8);
    let flag = cancel.clone();
    let walker = std::thread::spawn(move || {
        Walker {
            out: Batcher {
                sender,
                batch: Vec::new(),
                bytes: 0,
                last: Instant::now(),
            },
            cancel: flag,
            scope,
            exclude,
            next: 1,
            fatal: None,
        }
        .run()
    });
    let result = (|| -> Result<()> {
        let mut fatal = None;
        let mut saw_end = false;
        let mut seq = 0;
        let mut last = Instant::now();
        emit(Progress {
            scan: store::scan(&db, sid)?,
            sequence: seq,
        });
        for batch in receiver.iter() {
            let tx = db.transaction()?;
            for e in batch {
                match e {
                    Event::Entry(r) => {
                        let file = r.kind == "file" && !r.excluded;
                        let unknown = r.kind == "unknown";
                        let mut owner = r.id;
                        if file {
                            if let (Some(dev), Some(ino)) = (&r.device, &r.inode) {
                                tx.execute("INSERT OR IGNORE INTO file_identities(scan,device,inode,owner) VALUES(?1,?2,?3,?4)",params![sid,dev,ino,r.id])?;
                                owner=tx.query_row("SELECT owner FROM file_identities WHERE scan=?1 AND device=?2 AND inode=?3",params![sid,dev,ino],|row|row.get(0))?;
                            }
                        }
                        let counted = file && owner == r.id;
                        let display = r.path.to_string_lossy().into_owned();
                        let note = r.note;
                        let logical = r.logical;
                        let allocated = r.allocated;
                        tx.execute("INSERT INTO entries(scan,id,parent,depth,name,path,path_display,search,kind,device,inode,logical,allocated,known_l,known_a,unknown_l,unknown_a,observed_l,observed_a,files,canonical,excluded,note,modified,open) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
                    params![sid,r.id,r.parent,r.depth,r.path.file_name().unwrap_or(r.path.as_os_str()).to_string_lossy(),platform::path_bytes(&r.path),display,display.to_lowercase(),r.kind,r.device,r.inode,logical,allocated,
                        if counted{logical.unwrap_or(0)}else{0},if counted{allocated.unwrap_or(0)}else{0},
                        (unknown||(counted&&logical.is_none())) as i64,(unknown||(counted&&allocated.is_none())) as i64,
                        (counted&&logical.is_some()) as i64,(counted&&allocated.is_some()) as i64,file as i64,if file{Some(owner)}else{None},r.excluded,note,r.modified,(r.kind=="directory"&&!r.excluded) as i64])?;
                        tx.execute("UPDATE scans SET entries=entries+1,files=files+?2,excluded=excluded+?3,current_path=?4 WHERE id=?1",params![sid,file as i64,r.excluded as i64,display])?;
                    }
                    Event::Closed(id, incomplete) => {
                        tx.execute("UPDATE entries SET open=0,unknown_l=unknown_l+?3,unknown_a=unknown_a+?3,gaps=gaps+?3 WHERE scan=?1 AND id=?2",params![sid,id,incomplete as i64])?;
                    }
                    Event::Error(entry, path, e) => {
                        add_error(&tx, sid, entry, &path, &e)?;
                    }
                    Event::End(reason) => {
                        fatal = reason;
                        saw_end = true;
                    }
                }
            }
            tx.commit()?;
            if last.elapsed() >= Duration::from_millis(250) {
                seq += 1;
                emit(Progress {
                    scan: store::scan(&db, sid)?,
                    sequence: seq,
                });
                last = Instant::now();
            }
        }
        if !saw_end {
            fatal = Some("扫描工作线程意外中断。".into());
        }
        db.execute("UPDATE scans SET state='finalizing' WHERE id=?1", [sid])?;
        seq += 1;
        emit(Progress {
            scan: store::scan(&db, sid)?,
            sequence: seq,
        });
        db.execute("UPDATE entries SET unknown_l=unknown_l+open,unknown_a=unknown_a+open,gaps=gaps+open,open=0 WHERE scan=?1 AND open>0",[sid])?;
        aggregate(&mut db, sid)?;
        let previous = store::scan(&db, sid)?;
        let cancelled = cancel.load(Ordering::Relaxed);
        let state = if fatal.is_some() {
            "failed"
        } else if cancelled {
            "cancelled"
        } else if previous.errors != "0" {
            "completed_with_gaps"
        } else {
            "completed"
        };
        let has_result = previous.entries != "0" && (fatal.is_none() || previous.entries != "1");
        let tx = db.transaction()?;
        tx.execute("UPDATE scans SET state=?2,reason=?3,ended=?4,has_result=?5,current_path='' WHERE id=?1",params![sid,state,fatal,now(),has_result])?;
        if matches!(state, "completed" | "completed_with_gaps") {
            store::show(&tx, sid)?;
        }
        tx.commit()?;
        seq += 1;
        emit(Progress {
            scan: store::scan(&db, sid)?,
            sequence: seq,
        });
        let _ = db.execute_batch("PRAGMA wal_checkpoint(PASSIVE)");
        Ok(())
    })();
    if result.is_err() {
        cancel.store(true, Ordering::Relaxed);
    }
    drop(receiver);
    let _ = walker.join();
    result
}
fn add_error(
    db: &rusqlite::Connection,
    sid: &str,
    entry: Option<i64>,
    path: &str,
    error: &AppError,
) -> Result<()> {
    db.execute("INSERT INTO scan_errors(scan,entry,path_display,code,message,occurred) VALUES(?1,?2,?3,?4,?5,?6)",params![sid,entry,path,error.code,error.message,now()])?;
    db.execute("UPDATE scans SET errors=errors+1 WHERE id=?1", [sid])?;
    Ok(())
}
fn aggregate(db: &mut rusqlite::Connection, sid: &str) -> Result<()> {
    let depth: Option<i64> = db.query_row(
        "SELECT MAX(depth) FROM entries WHERE scan=?1 AND kind='directory'",
        [sid],
        |r| r.get(0),
    )?;
    for depth in (0..=depth.unwrap_or(0)).rev() {
        let mut after = 0;
        loop {
            let ids = {
                let mut q=db.prepare("SELECT id FROM entries WHERE scan=?1 AND kind='directory' AND depth=?2 AND id>?3 ORDER BY id LIMIT 100")?;
                let rows = q.query_map(params![sid, depth, after], |r| r.get::<_, i64>(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            if ids.is_empty() {
                break;
            }
            let tx = db.transaction()?;
            for id in ids {
                after = id;
                let mut values=tx.query_row("SELECT known_l,known_a,unknown_l,unknown_a,observed_l,observed_a,gaps,files FROM entries WHERE scan=?1 AND id=?2",params![sid,id],|r|{let mut a=[0i64;8];for(i,v)in a.iter_mut().enumerate(){*v=r.get(i)?;}Ok(a)})?;
                let mut overflow = false;
                {
                    let mut q=tx.prepare("SELECT known_l,known_a,unknown_l,unknown_a,observed_l,observed_a,gaps,files FROM entries WHERE scan=?1 AND parent=?2")?;
                    let mut rows = q.query(params![sid, id])?;
                    while let Some(row) = rows.next()? {
                        for (i, value) in values.iter_mut().enumerate() {
                            let n: i64 = row.get(i)?;
                            if let Some(sum) = value.checked_add(n) {
                                *value = sum;
                            } else {
                                overflow = true;
                            }
                        }
                    }
                }
                if overflow {
                    values[2] = values[2].saturating_add(1);
                    values[3] = values[3].saturating_add(1);
                    let p: String = tx.query_row(
                        "SELECT path_display FROM entries WHERE scan=?1 AND id=?2",
                        params![sid, id],
                        |r| r.get(0),
                    )?;
                    add_error(
                        &tx,
                        sid,
                        Some(id),
                        &p,
                        &AppError::new(
                            "size_overflow",
                            "目录累计量超出整数范围，显示的已知量不完整。",
                        ),
                    )?;
                }
                tx.execute("UPDATE entries SET known_l=?3,known_a=?4,unknown_l=?5,unknown_a=?6,observed_l=?7,observed_a=?8,gaps=?9,files=?10 WHERE scan=?1 AND id=?2",params![sid,id,values[0],values[1],values[2],values[3],values[4],values[5],values[6],values[7]])?;
            }
            tx.commit()?;
        }
    }
    Ok(())
}

pub fn create(db: &mut rusqlite::Connection, scope: &Scope, request: &str) -> Result<(Scan, bool)> {
    if request.len() > 128 || request.is_empty() {
        return Err(AppError::new("invalid_argument", "无效请求 ID。"));
    }
    if let Some((id, key)) = db
        .query_row(
            "SELECT id,scope_key FROM scans WHERE request=?1",
            [request],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        if key != scope.key {
            return Err(AppError::new(
                "request_conflict",
                "同一请求 ID 的范围不一致。",
            ));
        }
        return Ok((store::scan(db, &id)?, false));
    }
    let tx = db.transaction()?;
    let display = store::display_id(&tx)?.unwrap_or_default();
    tx.execute("DELETE FROM scans WHERE id<>?1", [display])?;
    let id = uuid::Uuid::new_v4().to_string();
    tx.execute("INSERT INTO scans(id,request,scope_key,root,path_display,device,inode,state,started) VALUES(?1,?2,?3,?4,?5,?6,?7,'preparing',?8)",params![id,request,scope.key,platform::path_bytes(&scope.path),scope.path_display,scope.device.to_string(),scope.inode.to_string(),now()])?;
    tx.commit()?;
    Ok((store::scan(db, &id)?, true))
}
