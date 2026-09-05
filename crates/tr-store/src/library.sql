CREATE TABLE asset (
    id TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    last_path_hint TEXT NOT NULL,
    rating INTEGER NOT NULL CHECK(typeof(rating)='integer' AND rating BETWEEN -1 AND 5),
    annotation TEXT NOT NULL CHECK(length(annotation)<=16384),
    revision INTEGER NOT NULL CHECK(revision>=0)
);
CREATE TABLE asset_location (
    path_key BLOB PRIMARY KEY,
    asset_id TEXT NOT NULL REFERENCES asset(id),
    content_hash TEXT NOT NULL
);
CREATE TABLE asset_revision (
    asset_id TEXT NOT NULL REFERENCES asset(id),
    revision INTEGER NOT NULL,
    annotation TEXT NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('initial','user','undo')),
    change_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(asset_id, revision)
);
CREATE TABLE effect_journal (
    effect_id TEXT PRIMARY KEY,
    asset_id TEXT NOT NULL REFERENCES asset(id),
    requested_revision INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK(kind='library_annotation'),
    state TEXT NOT NULL CHECK(state='committed'),
    UNIQUE(asset_id, requested_revision)
);
PRAGMA user_version=1;
