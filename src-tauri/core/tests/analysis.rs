#![cfg(unix)]
use spacelens_core::{model::*, platform, query, store, Engine};
use std::{
    fs,
    os::unix::{
        ffi::OsStringExt,
        fs::{symlink, MetadataExt, PermissionsExt},
    },
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    engine: Arc<Engine>,
    scope: Scope,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let tree = temp.path().join("files");
        fs::create_dir(&tree).unwrap();
        let db = temp.path().join("index/index.sqlite");
        let engine = Engine::new(db, vec![], |_| {}).unwrap();
        let scope = platform::scope(tree).unwrap();
        Self {
            temp,
            engine,
            scope,
        }
    }
    fn scan(&self, request: &str) -> Scan {
        let s = self.engine.start(self.scope.clone(), request).unwrap();
        wait(&self.engine, &s.id)
    }
    fn db(&self) -> spacelens_core::DbConnection {
        store::connect(&self.engine.database).unwrap()
    }
}
fn wait(engine: &Engine, id: &str) -> Scan {
    let start = Instant::now();
    while engine.active_id().is_some() {
        assert!(start.elapsed() < Duration::from_secs(30), "scan timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
    store::scan(&store::connect(&engine.database).unwrap(), id).unwrap()
}
fn q(scan: &str) -> Query {
    Query {
        scan_id: scan.into(),
        parent_id: Some("1".into()),
        metric: Metric::Allocated,
        search: String::new(),
        descending: true,
        cursor: None,
        limit: 100,
        group_token: None,
    }
}
fn find(db: &spacelens_core::DbConnection, sid: &str, name: &str) -> Entry {
    let mut query = q(sid);
    query.search = name.into();
    query::list(db, &query, "children")
        .unwrap()
        .items
        .into_iter()
        .find(|e| e.name == name)
        .unwrap()
}

#[test]
fn sparse_links_hidden_files_and_directory_totals() {
    let f = Fixture::new();
    let root = &f.scope.path;
    fs::create_dir(root.join("a")).unwrap();
    fs::create_dir(root.join("b")).unwrap();
    fs::write(root.join("a/data.bin"), vec![1u8; 8192]).unwrap();
    fs::hard_link(root.join("a/data.bin"), root.join("b/alias.bin")).unwrap();
    fs::write(root.join(".hidden"), b"yes").unwrap();
    fs::write(root.join("empty"), []).unwrap();
    let sparse = fs::File::create(root.join("sparse.raw")).unwrap();
    sparse.set_len(512 * 1024 * 1024).unwrap();
    fs::write(f.temp.path().join("outside"), vec![0u8; 4096]).unwrap();
    symlink(f.temp.path(), root.join("escape")).unwrap();
    let scan = f.scan("scan-1");
    assert_eq!(scan.state, "completed");
    assert_eq!(scan.excluded, "1");
    let db = f.db();
    let root_entry = store::entry(&db, &scan.id, 1).unwrap();
    assert_eq!(
        root_entry.known_logical,
        (8192 + 3 + 512 * 1024 * 1024u64).to_string()
    );
    let expected = [
        root.join("a/data.bin"),
        root.join(".hidden"),
        root.join("empty"),
        root.join("sparse.raw"),
    ]
    .iter()
    .map(|p| fs::metadata(p).unwrap().blocks() * 512)
    .sum::<u64>();
    assert_eq!(root_entry.known_allocated, expected.to_string());
    let sparse_entry = find(&db, &scan.id, "sparse.raw");
    assert!(
        sparse_entry
            .logical
            .as_ref()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > sparse_entry
                .allocated
                .as_ref()
                .unwrap()
                .parse::<u64>()
                .unwrap()
    );
    let rows = query::list(&db, &q(&scan.id), "large").unwrap().items;
    assert_eq!(rows.len(), 4);
    assert_eq!(root_entry.files, "5");
    let aliases: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM entries WHERE scan=?1 AND canonical<>id",
            [&scan.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(aliases, 1);
    let children = query::list(&db, &q(&scan.id), "children").unwrap().items;
    assert_eq!(
        children
            .iter()
            .map(|e| e.known_allocated.parse::<u64>().unwrap())
            .sum::<u64>(),
        expected
    );
}

#[test]
fn pagination_grouping_and_filter_boundaries() {
    let f = Fixture::new();
    for i in 0..215 {
        fs::write(f.scope.path.join(format!("item-{i:03}")), b"x").unwrap();
    }
    fs::write(f.scope.path.join("100%_done.txt"), b"literal").unwrap();
    let s = f.scan("pages");
    let db = f.db();
    let mut request = q(&s.id);
    request.limit = 37;
    let mut ids = Vec::new();
    loop {
        let page = query::list(&db, &request, "children").unwrap();
        ids.extend(page.items.into_iter().map(|e| e.id));
        if page.next_cursor.is_none() {
            break;
        }
        request.cursor = page.next_cursor;
    }
    assert_eq!(ids.len(), 216);
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 216);
    let first = query::list(&db, &request.clone_with_first(), "children").unwrap();
    let mut changed = request.clone_with_first();
    changed.cursor = first.next_cursor;
    changed.metric = Metric::Logical;
    assert_eq!(
        query::list(&db, &changed, "children").unwrap_err().code,
        "cursor_invalid"
    );
    let map = query::map(&db, &s.id, "1", Metric::Allocated).unwrap();
    assert_eq!(map.items.len(), 40);
    assert_eq!(map.other_count, "176");
    assert_eq!(
        map.items
            .iter()
            .map(|e| e.known_allocated.parse::<u64>().unwrap())
            .sum::<u64>()
            + map.other_bytes.parse::<u64>().unwrap(),
        map.total.parse::<u64>().unwrap()
    );
    let mut group = q(&s.id);
    group.group_token = map.group_token;
    let group_page = query::list(&db, &group, "children").unwrap();
    assert!(group_page
        .items
        .iter()
        .all(|e| !map.items.iter().any(|t| t.id == e.id)));
    let mut search = q(&s.id);
    search.search = "%_".into();
    let matches = query::list(&db, &search, "large").unwrap();
    assert_eq!(matches.items.len(), 1);
    assert_eq!(matches.items[0].name, "100%_done.txt");
    group.metric = Metric::Logical;
    assert_eq!(
        query::list(&db, &group, "children").unwrap_err().code,
        "cursor_invalid"
    );
}
trait First {
    fn clone_with_first(&self) -> Self;
}
impl First for Query {
    fn clone_with_first(&self) -> Self {
        let mut q = self.clone();
        q.cursor = None;
        q
    }
}

#[test]
fn permission_gaps_are_not_empty_directories() {
    let f = Fixture::new();
    let p = f.scope.path.join("private");
    fs::create_dir(&p).unwrap();
    fs::write(p.join("secret"), b"secret").unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0)).unwrap();
    let s = f.scan("denied");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(s.state, "completed_with_gaps");
    let db = f.db();
    let entry = find(&db, &s.id, "private");
    assert!(entry.unknown(Metric::Allocated));
    assert_ne!(entry.gaps, "0");
    assert!(store::entry(&db, &s.id, 1)
        .unwrap()
        .unknown(Metric::Allocated));
    assert!(!query::errors(&db, &s.id, None).unwrap().items.is_empty());
}

#[test]
fn cancellation_retains_previous_result_and_is_idempotent() {
    let f = Fixture::new();
    fs::write(f.scope.path.join("stable"), b"keep").unwrap();
    let old = f.scan("first");
    let gate = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let g = gate.clone();
    let r = release.clone();
    let engine = Engine::new(f.engine.database.clone(), vec![], move |p| {
        if p.scan.state == "scanning" && p.sequence == 0 {
            g.store(true, Ordering::SeqCst);
            while !r.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    })
    .unwrap();
    let s = engine.start(f.scope.clone(), "cancel").unwrap();
    let start = Instant::now();
    while !gate.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(engine.start(f.scope.clone(), "cancel").unwrap().id, s.id);
    assert_eq!(
        engine.start(f.scope.clone(), "another").unwrap_err().code,
        "scan_busy"
    );
    engine.cancel(&s.id).unwrap();
    release.store(true, Ordering::SeqCst);
    let cancelled = wait(&engine, &s.id);
    assert_eq!(cancelled.state, "cancelled");
    let db = f.db();
    assert_eq!(
        store::display_id(&db).unwrap().as_deref(),
        Some(old.id.as_str())
    );
    assert_eq!(engine.cancel(&s.id).unwrap().state, "cancelled");
    assert!(cancelled.has_result);
}

#[test]
fn lossless_paths_and_replaced_identity() {
    let f = Fixture::new();
    // APFS can reject invalid UTF-8 filenames; test the lossless codec separately.
    let raw_name = std::ffi::OsString::from_vec(vec![b'x', 0xff]);
    let raw_path = std::path::PathBuf::from(raw_name);
    assert_eq!(
        platform::from_bytes(platform::path_bytes(&raw_path)),
        raw_path
    );
    let p = f.scope.path.join("旅行📷.bin");
    fs::write(&p, b"data").unwrap();
    let s = f.scan("raw");
    let db = f.db();
    let item = query::list(&db, &q(&s.id), "children")
        .unwrap()
        .items
        .remove(0);
    let (root, raw, identity) = store::native_path(&db, &s.id, &item.id).unwrap();
    assert_eq!(
        platform::verified_path(&root, raw.clone(), identity).unwrap(),
        p
    );
    fs::rename(&p, f.scope.path.join("old")).unwrap();
    fs::write(&p, b"replacement").unwrap();
    assert_eq!(
        platform::verified_path(&root, raw, identity)
            .unwrap_err()
            .code,
        "entry_replaced"
    );
}

#[test]
fn empty_and_excluded_directories_are_distinct() {
    let f = Fixture::new();
    fs::create_dir(f.scope.path.join("empty")).unwrap();
    let excluded = f.scope.path.join("index");
    fs::create_dir(&excluded).unwrap();
    fs::write(excluded.join("database"), b"skip").unwrap();
    let engine = Engine::new(f.engine.database.clone(), vec![excluded], |_| {}).unwrap();
    let s = engine.start(f.scope.clone(), "excluded").unwrap();
    let s = wait(&engine, &s.id);
    let db = f.db();
    let empty = find(&db, &s.id, "empty");
    assert!(!empty.unknown(Metric::Allocated));
    assert!(!empty.excluded);
    assert_eq!(empty.known_allocated, "0");
    assert!(find(&db, &s.id, "index").excluded);
    assert_eq!(s.files, "0");
}

#[test]
fn recovery_two_generation_retention_and_settings() {
    let f = Fixture::new();
    let first = f.scan("first");
    let second = f.scan("second");
    let third = f.scan("third");
    let mut db = f.db();
    assert_eq!(store::scans(&db).unwrap().len(), 2);
    assert_eq!(
        store::scan(&db, &first.id).unwrap_err().code,
        "result_expired"
    );
    assert_eq!(store::display_id(&db).unwrap(), Some(third.id.clone()));
    db.execute(
        "UPDATE scans SET state='scanning',has_result=0 WHERE id=?1",
        [&second.id],
    )
    .unwrap();
    let mut settings = store::settings(&db).unwrap();
    settings.unit = "decimal".into();
    let updated = store::save_settings(&mut db, &settings).unwrap();
    assert_eq!(updated.version, 2);
    assert_eq!(
        store::save_settings(&mut db, &settings).unwrap_err().code,
        "settings_conflict"
    );
    drop(db);
    let _restored = Engine::new(f.engine.database.clone(), vec![], |_| {}).unwrap();
    let db = f.db();
    assert_eq!(store::scan(&db, &second.id).unwrap().state, "interrupted");
    assert_eq!(store::display_id(&db).unwrap(), Some(third.id));
    assert_eq!(store::settings(&db).unwrap().unit, "decimal");
}

#[test]
fn chosen_root_cannot_be_replaced() {
    let f = Fixture::new();
    let root = f.scope.path.clone();
    fs::rename(&root, f.temp.path().join("original")).unwrap();
    fs::create_dir(&root).unwrap();
    assert_eq!(
        f.engine.start(f.scope.clone(), "replace").unwrap_err().code,
        "scope_invalid"
    );
}

#[test]
#[ignore = "受控规模性能检查，手动运行以避免每次创建大量文件"]
fn scan_twenty_thousand_files() {
    let f = Fixture::new();
    for d in 0..100 {
        let dir = f.scope.path.join(format!("d{d}"));
        fs::create_dir(&dir).unwrap();
        for n in 0..200 {
            fs::write(dir.join(format!("f{n}")), []).unwrap();
        }
    }
    let start = Instant::now();
    let s = f.scan("scale");
    assert_eq!(s.files, "20000");
    assert_eq!(s.state, "completed");
    eprintln!("20,000 files scanned in {:?}", start.elapsed());
}
