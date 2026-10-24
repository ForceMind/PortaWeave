PRAGMA application_id = 1347898693;
PRAGMA user_version = 2;
CREATE TABLE config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    quota INTEGER NOT NULL CHECK (quota >= 0),
    format TEXT NOT NULL CHECK (format = 'experimental-m1h-metadata-v2')
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
CREATE TABLE volume_identity (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    uuid BLOB NOT NULL CHECK (length(uuid) = 16),
    capabilities INTEGER NOT NULL CHECK (capabilities = 1)
) STRICT;
CREATE TABLE object_metadata (
    object INTEGER PRIMARY KEY REFERENCES objects(id) ON DELETE CASCADE,
    uid INTEGER NOT NULL CHECK (uid BETWEEN 0 AND 4294967295),
    gid INTEGER NOT NULL CHECK (gid BETWEEN 0 AND 4294967295),
    mode INTEGER NOT NULL CHECK (mode BETWEEN 0 AND 4095),
    birth_sec INTEGER NOT NULL,
    birth_nsec INTEGER NOT NULL CHECK (birth_nsec BETWEEN 0 AND 999999999),
    access_sec INTEGER NOT NULL,
    access_nsec INTEGER NOT NULL CHECK (access_nsec BETWEEN 0 AND 999999999),
    modify_sec INTEGER NOT NULL,
    modify_nsec INTEGER NOT NULL CHECK (modify_nsec BETWEEN 0 AND 999999999),
    change_sec INTEGER NOT NULL,
    change_nsec INTEGER NOT NULL CHECK (change_nsec BETWEEN 0 AND 999999999)
) STRICT;
