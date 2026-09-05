CREATE TABLE item (
    path_key BLOB PRIMARY KEY,
    asset_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    size INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
PRAGMA user_version=1;
