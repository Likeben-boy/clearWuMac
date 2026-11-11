#[cfg(not(unix))]
compile_error!("当前版本实现 macOS/Unix 扫描；Windows 平台适配尚未完成。");
pub mod model;
pub mod platform;
pub mod query;
pub mod store;
pub type DbConnection = rusqlite::Connection;
mod scan;
use model::*;
use rusqlite::params;
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub struct Engine {
    pub database: PathBuf,
    exclude: Vec<PathBuf>,
    active: Mutex<Option<(String, Arc<AtomicBool>)>>,
    emit: Arc<dyn Fn(Progress) + Send + Sync>,
}
impl Engine {
    pub fn new(
        database: PathBuf,
        exclude: Vec<PathBuf>,
        emit: impl Fn(Progress) + Send + Sync + 'static,
    ) -> Result<Arc<Self>> {
        store::initialize(&database)?;
        Ok(Arc::new(Self {
            database,
            exclude,
            active: Mutex::new(None),
            emit: Arc::new(emit),
        }))
    }
    pub fn active_id(&self) -> Option<String> {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .map(|(id, _)| id.clone())
    }
    pub fn start(self: &Arc<Self>, scope: Scope, request: &str) -> Result<Scan> {
        let mut active = self.active.lock().unwrap();
        let mut db = store::connect(&self.database)?;
        // Check idempotence before busy status, without deleting any retained record.
        use rusqlite::OptionalExtension;
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
                    "同一请求 ID 不可更换范围。",
                ));
            }
            return store::scan(&db, &id);
        }
        if active.is_some() {
            return Err(AppError::new(
                "scan_busy",
                "已有扫描正在运行，请等待或取消。",
            ));
        }
        platform::check_scope(&scope)?;
        if self.exclude.iter().any(|p| scope.path.starts_with(p)) {
            return Err(AppError::new(
                "scope_invalid",
                "应用自身索引目录不能作为扫描范围。",
            ));
        }
        if let Some(parent) = self.database.parent() {
            if platform::volume(parent, false)?
                .available
                .parse::<u64>()
                .unwrap_or(0)
                < 256 * 1024 * 1024
            {
                return Err(AppError::new(
                    "database_full",
                    "应用索引所在磁盘不足 256 MiB，请先释放空间。",
                ));
            }
        }
        let (record, created) = scan::create(&mut db, &scope, request)?;
        if !created {
            return Ok(record);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some((record.id.clone(), cancel.clone()));
        let this = self.clone();
        let sid = record.id.clone();
        std::thread::spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                scan::run(
                    &this.database,
                    &sid,
                    scope,
                    this.exclude.clone(),
                    cancel,
                    &*this.emit,
                )
            }));
            let error = match outcome {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e),
                Err(_) => Some(AppError::new("scan_failed", "扫描线程意外中断。")),
            };
            if let Some(error) = error {
                if let Ok(db) = store::connect(&this.database) {
                    let _=db.execute("UPDATE scans SET state='failed',reason=?2,ended=?3,has_result=0 WHERE id=?1",params![sid,error.message,now()]);
                }
            }
            *this.active.lock().unwrap() = None;
            if let Ok(db) = store::connect(&this.database) {
                if let Ok(scan) = store::scan(&db, &sid) {
                    (this.emit)(Progress {
                        scan,
                        sequence: u64::MAX,
                    });
                }
            }
        });
        Ok(record)
    }
    pub fn cancel(&self, id: &str) -> Result<Scan> {
        let active = self.active.lock().unwrap();
        let db = store::connect(&self.database)?;
        if let Some((active_id, flag)) = &*active {
            if active_id == id {
                flag.store(true, Ordering::Relaxed);
                db.execute("UPDATE scans SET state='cancelling' WHERE id=?1 AND state IN ('preparing','scanning')",[id])?;
            }
        }
        store::scan(&db, id)
    }
}
