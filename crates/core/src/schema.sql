PRAGMA application_id = 1347898693;
PRAGMA user_version = 1;
CREATE TABLE config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    quota INTEGER NOT NULL CHECK (quota >= 0),
    format TEXT NOT NULL CHECK (format = 'experimental-m1a-block-v1')
) STRICT;
CREATE TABLE objects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind INTEGER NOT NULL CHECK (kind IN (0, 1)),
    size INTEGER NOT NULL DEFAULT 0 CHECK (size >= 0),
    generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0)
) STRICT;
CREATE TABLE entries (
    parent INTEGER NOT NULL REFERENCES objects(id),
    name_key TEXT NOT NULL,
    display_name TEXT NOT NULL,
    object INTEGER NOT NULL REFERENCES objects(id),
    PRIMARY KEY (parent, name_key)
) STRICT;
CREATE INDEX entries_object ON entries(object);
CREATE TABLE segments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bytes INTEGER NOT NULL DEFAULT 0 CHECK (bytes >= 0)
) STRICT;
CREATE TABLE blocks (
    object INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    block INTEGER NOT NULL CHECK (block >= 0),
    segment INTEGER NOT NULL REFERENCES segments(id),
    offset INTEGER NOT NULL CHECK (offset >= 24),
    checksum BLOB NOT NULL CHECK (length(checksum) = 32),
    PRIMARY KEY (object, block)
) STRICT;
CREATE INDEX blocks_segment ON blocks(segment);
INSERT INTO objects(id, kind) VALUES (1, 1);
