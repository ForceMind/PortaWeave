#include "portaweave_native.h"
#include <stdio.h>
#include <string.h>

/* Real C executable + Rust cdylib against a NEW CI scratch directory. */
#define REQUIRE(call) do { \
    int status = (call); \
    if (status != PW_OK) { fprintf(stderr, "%s returned %d\n", #call, status); return 1; } \
} while (0)

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    const char *path = argv[1];
    uint64_t volume = 0, handle = 0;
    int64_t node = 0, found = 0;
    size_t count = 0;
    uint8_t buffer[64] = {0};
    if (pw_abi_version() != 1 || pw_root_id() != 1) return 3;
    REQUIRE(pw_volume_create((const uint8_t *)path, strlen(path), 65536, &volume));
    REQUIRE(pw_file_create(volume, 1, (const uint8_t *)"native.txt", 10, &node));
    REQUIRE(pw_file_open(volume, node, &handle));
    REQUIRE(pw_file_write(volume, handle, 0, (const uint8_t *)"hello", 5, &count));
    if (count != 5) return 4;
    PwSpace space = {0};
    REQUIRE(pw_volume_space(volume, &space));
    if (space.logical_quota_bytes != 65536 || space.logical_used_bytes != 5 ||
        space.apparent_free_bytes > 65531 || space.apparent_free_bytes >
        space.host_data_admission_bytes) return 9;
    PwCheckpoint checkpoint = {0};
    REQUIRE(pw_catalog_checkpoint(volume, &checkpoint));
    if (checkpoint.checkpointed_frames != checkpoint.wal_frames) return 10;
    REQUIRE(pw_file_read(volume, handle, 0, buffer, sizeof(buffer), &count));
    if (count != 5 || memcmp(buffer, "hello", 5) != 0) return 5;
    if (pw_volume_close(volume) != PW_BUSY) return 6;
    REQUIRE(pw_file_close(volume, handle));
    REQUIRE(pw_rename(volume, 1, (const uint8_t *)"native.txt", 10,
                      1, (const uint8_t *)"renamed.txt", 11, 0));
    REQUIRE(pw_volume_close(volume));
    REQUIRE(pw_volume_open((const uint8_t *)path, strlen(path), &volume));
    REQUIRE(pw_lookup(volume, 1, (const uint8_t *)"renamed.txt", 11, &found));
    if (node != found) return 7;
    PwStat stat = {0};
    REQUIRE(pw_stat(volume, found, &stat));
    if (stat.kind != 0 || stat.size != 5 || stat.links != 1) return 8;
    REQUIRE(pw_remove(volume, 1, (const uint8_t *)"renamed.txt", 11, 0));
    REQUIRE(pw_volume_close(volume));
    puts("NATIVE_C_ABI_OK");
    return 0;
}
