CREATE TABLE IF NOT EXISTS scans (
 id TEXT PRIMARY KEY, request TEXT UNIQUE NOT NULL, scope_key TEXT NOT NULL,
 root BLOB NOT NULL, path_display TEXT NOT NULL, device TEXT NOT NULL, inode TEXT NOT NULL,
 state TEXT NOT NULL, reason TEXT, started INTEGER NOT NULL, ended INTEGER,
 entries INTEGER NOT NULL DEFAULT 0, files INTEGER NOT NULL DEFAULT 0,
 errors INTEGER NOT NULL DEFAULT 0, excluded INTEGER NOT NULL DEFAULT 0,
 has_result INTEGER NOT NULL DEFAULT 0, current_path TEXT NOT NULL DEFAULT '',
 method TEXT NOT NULL DEFAULT 'macos_stat_blocks_v1'
);
CREATE TABLE IF NOT EXISTS entries (
 scan TEXT NOT NULL REFERENCES scans(id) ON DELETE CASCADE,
 id INTEGER NOT NULL, parent INTEGER, depth INTEGER NOT NULL, name TEXT NOT NULL,
 path BLOB NOT NULL, path_display TEXT NOT NULL, search TEXT NOT NULL,
 kind TEXT NOT NULL, device TEXT, inode TEXT, logical INTEGER, allocated INTEGER,
 known_l INTEGER NOT NULL DEFAULT 0, known_a INTEGER NOT NULL DEFAULT 0,
 unknown_l INTEGER NOT NULL DEFAULT 0, unknown_a INTEGER NOT NULL DEFAULT 0,
 observed_l INTEGER NOT NULL DEFAULT 0, observed_a INTEGER NOT NULL DEFAULT 0,
 gaps INTEGER NOT NULL DEFAULT 0, files INTEGER NOT NULL DEFAULT 0,
 canonical INTEGER, excluded INTEGER NOT NULL DEFAULT 0, note TEXT, modified INTEGER,
 open INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(scan,id), FOREIGN KEY(scan,parent) REFERENCES entries(scan,id)
);
CREATE TABLE IF NOT EXISTS file_identities (
 scan TEXT NOT NULL REFERENCES scans(id) ON DELETE CASCADE,
 device TEXT NOT NULL, inode TEXT NOT NULL, owner INTEGER NOT NULL,
 PRIMARY KEY(scan,device,inode)
);
CREATE TABLE IF NOT EXISTS scan_errors (
 id INTEGER PRIMARY KEY, scan TEXT NOT NULL REFERENCES scans(id) ON DELETE CASCADE,
 entry INTEGER, path_display TEXT NOT NULL, code TEXT NOT NULL, message TEXT NOT NULL, occurred INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS app_state (key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS entry_parent ON entries(scan,parent,id);
CREATE INDEX IF NOT EXISTS entry_depth ON entries(scan,depth,id);
CREATE INDEX IF NOT EXISTS entry_allocated ON entries(scan,parent,known_a DESC,id);
CREATE INDEX IF NOT EXISTS entry_logical ON entries(scan,parent,known_l DESC,id);
CREATE INDEX IF NOT EXISTS large_allocated ON entries(scan,kind,known_a DESC,id);
CREATE INDEX IF NOT EXISTS large_logical ON entries(scan,kind,known_l DESC,id);
CREATE INDEX IF NOT EXISTS aliases ON entries(scan,canonical,id);
CREATE INDEX IF NOT EXISTS errors_scan ON scan_errors(scan,id);
PRAGMA user_version = 1;
