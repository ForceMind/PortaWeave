/* PortaWeave M0-A experimental native C API. Not a mount, not stable ABI. */
#ifndef PORTAWEAVE_H
#define PORTAWEAVE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

/* Session/handle IDs are process-local and NEVER recycled in a process. */
#define PW_OK 0
#define PW_INVALID_ARGUMENT (-1)
#define PW_NOT_FOUND (-2)
#define PW_ALREADY_EXISTS (-3)
#define PW_BAD_HANDLE (-4)
#define PW_NOT_DIRECTORY (-5)
#define PW_IS_DIRECTORY (-6)
#define PW_NOT_EMPTY (-7)
#define PW_NO_SPACE (-8)
#define PW_RESOURCE_LIMIT (-9)
#define PW_BUSY (-10)
#define PW_UNSUPPORTED (-11)
#define PW_CORRUPT (-12)
#define PW_READ_ONLY (-13)
#define PW_IO_ERROR (-14)
#define PW_INTERNAL (-127)
#define PW_ROOT_ID UINT64_C(1)
#define PW_FILE 0u
#define PW_DIRECTORY 1u
#define PW_MAINTENANCE_HEALTHY 0u
#define PW_MAINTENANCE_DEFERRED_SPACE 1u
#define PW_MAINTENANCE_FAILED 2u

typedef struct PwFileStat {
    uint64_t id;
    uint32_t kind;
    uint64_t size;
    uint64_t links;
    uint64_t generation;
} PwFileStat;
typedef struct PwHealth {
    uint8_t write_protected;
    uint32_t maintenance;
} PwHealth;

/* All functions return PW_OK or a negative PW_* error code.
 * C strings for paths must be valid UTF-8 NUL-terminated strings.
 * Names are non-NUL-terminated UTF-8 byte ranges, up to 255 bytes.
 * Output pointers and nonempty input/output buffers must remain valid during
 * the call and may not overlap other active buffer regions.
 * The caller must not concurrently close a session while submitting new I/O.
 * Disk integrity and durability are NOT certified. No fsync, ACL, symlink,
 * enumeration, notification or mount operations are provided by this ABI.
 */
int32_t pw_volume_create(const char *path_utf8, uint64_t quota, uint64_t *out_session);
int32_t pw_volume_open(const char *path_utf8, uint64_t *out_session);
int32_t pw_volume_close(uint64_t session);
int32_t pw_health(uint64_t session, PwHealth *out_health);
int32_t pw_stat(uint64_t session, uint64_t object_id, PwFileStat *out_stat);
int32_t pw_lookup(uint64_t session, uint64_t parent,
                  const uint8_t *name_utf8, size_t name_len, uint64_t *out_id);
int32_t pw_create_file(uint64_t session, uint64_t parent,
                       const uint8_t *name_utf8, size_t name_len, uint64_t *out_id);
int32_t pw_mkdir(uint64_t session, uint64_t parent,
                 const uint8_t *name_utf8, size_t name_len, uint64_t *out_id);
int32_t pw_open_file(uint64_t session, uint64_t object_id, uint64_t *out_handle);
int32_t pw_close_file(uint64_t session, uint64_t handle);
int32_t pw_read(uint64_t session, uint64_t handle, uint64_t offset,
                uint8_t *buffer, size_t capacity, size_t *out_bytes);
int32_t pw_write(uint64_t session, uint64_t handle, uint64_t offset,
                 const uint8_t *buffer, size_t len, size_t *out_bytes);
int32_t pw_truncate(uint64_t session, uint64_t handle, uint64_t size);
int32_t pw_unlink(uint64_t session, uint64_t parent,
                  const uint8_t *name_utf8, size_t name_len);
int32_t pw_rmdir(uint64_t session, uint64_t parent,
                 const uint8_t *name_utf8, size_t name_len);
int32_t pw_rename(uint64_t session, uint64_t from_parent,
                  const uint8_t *from_utf8, size_t from_len,
                  uint64_t to_parent, const uint8_t *to_utf8,
                  size_t to_len, uint8_t replace_existing);
int32_t pw_hard_link(uint64_t session, uint64_t target_object, uint64_t parent,
                     const uint8_t *name_utf8, size_t name_len);
#ifdef __cplusplus
}
#endif
#endif
