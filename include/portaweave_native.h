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

uint32_t pw_abi_version(void);
int64_t pw_root_id(void);
int32_t pw_volume_create(const uint8_t *path, size_t path_len, uint64_t quota, uint64_t *out_volume);
int32_t pw_volume_open(const uint8_t *path, size_t path_len, uint64_t *out_volume);
int32_t pw_volume_close(uint64_t volume);
int32_t pw_stat(uint64_t volume, int64_t object, PwStat *out_stat);
int32_t pw_lookup(uint64_t volume, int64_t parent, const uint8_t *name, size_t name_len, int64_t *out_object);
int32_t pw_file_create(uint64_t volume, int64_t parent, const uint8_t *name, size_t name_len, int64_t *out_object);
int32_t pw_directory_create(uint64_t volume, int64_t parent, const uint8_t *name, size_t name_len, int64_t *out_object);
int32_t pw_file_open(uint64_t volume, int64_t object, uint64_t *out_file);
int32_t pw_file_close(uint64_t volume, uint64_t file);
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
int32_t pw_health(uint64_t volume, uint32_t *out_maintenance, uint32_t *out_protected);
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
