#ifndef PORTAWEAVE_NATIVE_H
#define PORTAWEAVE_NATIVE_H
/* Experimental v1 ABI. THIS IS NOT AN OS FILESYSTEM MOUNT.
 * UTF-8 pointer+len inputs MUST remain valid throughout calls. All out
 * pointers must be writable/aligned and must not alias an input buffer.
 * Failed calls do not guarantee rollback if a SQLite commit already began.
 * Call volume_close only after file and directory cursor handles are closed.
 * API is process-local. Engine owner lock permits only one opener per volume.
 */
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

enum {
  PW_OK = 0, PW_END = 1, PW_INVALID = -1, PW_NOT_FOUND = -2,
  PW_EXISTS = -3, PW_NOT_DIR = -4, PW_IS_DIR = -5, PW_NOT_EMPTY = -6,
  PW_BUSY = -7, PW_NO_SPACE = -8, PW_QUOTA = -9, PW_READ_ONLY = -10,
  PW_UNSUPPORTED = -11, PW_CORRUPT = -12, PW_IO = -13, PW_RESOURCE = -14,
  PW_INTERNAL = -15, PW_BUFFER_TOO_SMALL = -16
};

typedef struct PwStat {
  uint32_t kind;        /* 0=file, 1=directory */
  uint64_t size;
  uint64_t links;
  uint64_t generation;
} PwStat;

typedef struct PwSpace {
  uint64_t logical_quota_bytes;       /* max virtual logical size */
  uint64_t logical_used_bytes;        /* includes still-open unlinked objects */
  uint64_t host_available_bytes;      /* snapshot, not reserved */
  uint64_t host_data_admission_bytes; /* excludes soft headroom */
  uint64_t apparent_free_bytes;       /* min(logical, host allowance) */
} PwSpace;
typedef struct PwCheckpoint {
  uint64_t wal_frames;
  uint64_t checkpointed_frames;
} PwCheckpoint;
typedef struct PwStorageSync {
  uint64_t segment_files;
  uint64_t wal_frames;
  uint64_t checkpointed_frames;
} PwStorageSync;
typedef struct PwCreateAttributes {
  uint32_t uid, gid, mode, reserved; /* mode <= 07777; reserved must be zero */
} PwCreateAttributes;
typedef struct PwTimestamp {
  int64_t seconds;
  uint32_t nanoseconds, reserved; /* fraction < 1000000000, reserved zero */
} PwTimestamp;
typedef struct PwMetadata {
  uint32_t uid, gid, mode, reserved;
  PwTimestamp created, accessed, modified, changed;
} PwMetadata;
enum {
  PW_META_UID = 1, PW_META_GID = 2, PW_META_MODE = 4,
  PW_META_CREATED = 8, PW_META_ACCESSED = 16, PW_META_MODIFIED = 32
};
typedef struct PwMetadataUpdate {
  uint64_t valid; /* only bits above; change time is maintained by the core */
  uint32_t uid, gid, mode, reserved;
  PwTimestamp created, accessed, modified;
} PwMetadataUpdate;
typedef struct PwVolumeIdentity {
  uint8_t uuid[16]; /* persistent UUID v4; not a credential or secret */
  uint64_t capabilities; /* currently 1: POSIX metadata storage */
} PwVolumeIdentity;

uint32_t pw_abi_version(void);
int64_t pw_root_id(void);
int32_t pw_volume_create(const uint8_t *path, size_t path_len, uint64_t quota, uint64_t *out_volume);
/* Explicit NEW experimental v2 container. Existing v1 is never upgraded.
 * Metadata APIs are trusted storage operations, NOT permission enforcement or
 * OS caller authentication. All reserved fields must be zero. Outputs remain
 * unchanged on failure. v1 metadata operations return PW_UNSUPPORTED.
 */
int32_t pw_volume_create_with_metadata(const uint8_t *path, size_t path_len, uint64_t quota,
                                      const PwCreateAttributes *attributes, uint64_t *out_volume);
int32_t pw_volume_identity(uint64_t volume, PwVolumeIdentity *out_identity);
int32_t pw_metadata_get(uint64_t volume, int64_t object, PwMetadata *out_metadata);
int32_t pw_metadata_set(uint64_t volume, int64_t object, const PwMetadataUpdate *update);
int32_t pw_node_create_with_metadata(uint64_t volume, int64_t parent, const uint8_t *name, size_t name_len,
                                    uint8_t directory, const PwCreateAttributes *attributes, int64_t *out_object);
int32_t pw_volume_open(const uint8_t *path, size_t path_len, uint64_t *out_volume);
int32_t pw_volume_close(uint64_t volume);
int32_t pw_stat(uint64_t volume, int64_t object, PwStat *out_stat);
int32_t pw_lookup(uint64_t volume, int64_t parent, const uint8_t *name, size_t name_len, int64_t *out_object);
/* Stored spelling for this alias; on BUFFER_TOO_SMALL only out_name_len changes. */
int32_t pw_lookup_entry(uint64_t volume, int64_t parent, const uint8_t *name, size_t name_len,
                       uint8_t *name_buffer, size_t capacity, size_t *out_name_len, int64_t *out_object);
/* Logical directory parent; root returns itself, never a backing-host parent. */
int32_t pw_parent_directory(uint64_t volume, int64_t object, int64_t *out_parent);
int32_t pw_file_create(uint64_t volume, int64_t parent, const uint8_t *name, size_t name_len, int64_t *out_object);
int32_t pw_directory_create(uint64_t volume, int64_t parent, const uint8_t *name, size_t name_len, int64_t *out_object);
int32_t pw_file_open(uint64_t volume, int64_t object, uint64_t *out_file);
int32_t pw_file_close(uint64_t volume, uint64_t file);
/* Process-local immutable range leases, NOT OS mmap. The requested interval
 * must fit in signed 64-bit file offsets. Reads use absolute file offsets and
 * must fit within the lease. Original EOF is retained. Closing the source file
 * does not invalidate a lease; volume_close returns BUSY until all leases close.
 * Owner-wide view/index budgets apply. On failure output values are unchanged.
 */
int32_t pw_read_view_begin(uint64_t volume, uint64_t file, uint64_t offset, uint64_t length, uint64_t *out_view);
int32_t pw_read_view_read(uint64_t volume, uint64_t view, uint64_t offset, uint8_t *buffer, size_t capacity, size_t *out_read);
int32_t pw_read_view_close(uint64_t volume, uint64_t view);
int32_t pw_file_read(uint64_t volume, uint64_t file, uint64_t offset, uint8_t *buffer, size_t capacity, size_t *out_read);
int32_t pw_file_write(uint64_t volume, uint64_t file, uint64_t offset, const uint8_t *data, size_t length, size_t *out_written);
int32_t pw_file_truncate(uint64_t volume, uint64_t file, uint64_t size);
int32_t pw_remove(uint64_t volume, int64_t parent, const uint8_t *name, size_t name_len, uint8_t directory);
int32_t pw_rename(uint64_t volume, int64_t from_parent, const uint8_t *from_name, size_t from_len,
                  int64_t to_parent, const uint8_t *to_name, size_t to_len, uint8_t replace);
int32_t pw_hard_link(uint64_t volume, int64_t object, int64_t parent, const uint8_t *name, size_t name_len);
int32_t pw_dir_begin(uint64_t volume, int64_t parent, uint64_t *out_cursor);
int32_t pw_dir_next(uint64_t volume, uint64_t cursor, uint8_t *name_buf, size_t capacity, size_t *out_name_len, int64_t *out_object);
int32_t pw_dir_close(uint64_t volume, uint64_t cursor);
/* Same snapshot, inclusive end position; invalid positions leave it unchanged. */
int32_t pw_dir_seek(uint64_t volume, uint64_t cursor, uint64_t position);
int32_t pw_health(uint64_t volume, uint32_t *out_maintenance, uint32_t *out_protected);
int32_t pw_volume_space(uint64_t volume, PwSpace *out_space);
/* Catalog FULL WAL checkpoint only. Never treat as filesystem fsync. */
int32_t pw_catalog_checkpoint(uint64_t volume, PwCheckpoint *out_result);
/* Backend host-file + directory barrier (Unix). Native/OS pending writes must
 * first be drained by the adapter. NOT safe eject or hardware certification.
 * Windows returns PW_UNSUPPORTED; no successful no-op directory flush.
 * Does not close handles/leases or run GC. Failed output remains unchanged.
 */
int32_t pw_storage_synchronize(uint64_t volume, PwStorageSync *out_result);
/* health: 0=healthy, 1=compaction deferred (space), 2=failed.
 * Health MUST be checked separately after a successful write.
 *
 * Not provided: OS mount/unmount, generic fsync, OS permissions, symbolic
 * links, native mmap, watch notifications, ACL/ADS/xattr, cross-process
 * share/delete modes, Windows NTSTATUS or POSIX errno mapping.
 * DO NOT forward unsupported platform operations as successful.
 */
#ifdef __cplusplus
}
#endif
#endif /* PORTAWEAVE_NATIVE_H */
