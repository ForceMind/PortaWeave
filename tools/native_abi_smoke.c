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
    uint64_t volume = 0, handle = 0, view = 0;
    int64_t node = 0, found = 0;
    size_t count = 0;
    uint8_t buffer[64] = {0};
    if (pw_abi_version() != 1 || pw_root_id() != 1) return 3;
    REQUIRE(pw_volume_create((const uint8_t *)path, strlen(path), 65536, &volume));
    PwVolumeIdentity identity = {{0}, 99};
    PwMetadata metadata = {0};
    metadata.mode = 99;
    if (pw_volume_identity(volume, &identity) != PW_UNSUPPORTED || identity.capabilities != 99 ||
        pw_metadata_get(volume, 1, &metadata) != PW_UNSUPPORTED || metadata.mode != 99) return 20;
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
    REQUIRE(pw_read_view_begin(volume, handle, 1, 3, &view));
    count = 99;
    if (pw_read_view_read(volume, view, 0, buffer, 1, &count) != PW_INVALID ||
        count != 99) return 11;
    REQUIRE(pw_file_write(volume, handle, 0, (const uint8_t *)"HELLO", 5, &count));
    if (pw_volume_close(volume) != PW_BUSY) return 6;
    REQUIRE(pw_file_close(volume, handle));
    REQUIRE(pw_rename(volume, 1, (const uint8_t *)"native.txt", 10,
                      1, (const uint8_t *)"renamed.txt", 11, 0));
    REQUIRE(pw_read_view_read(volume, view, 1, buffer, 3, &count));
    if (count != 3 || memcmp(buffer, "ell", 3) != 0) return 12;
    PwStorageSync sync = {99, 98, 97};
#ifdef _WIN32
    if (pw_storage_synchronize(volume, &sync) != PW_UNSUPPORTED ||
        sync.segment_files != 99 || sync.wal_frames != 98 ||
        sync.checkpointed_frames != 97) return 15;
#else
    REQUIRE(pw_storage_synchronize(volume, &sync));
    if (!sync.segment_files || sync.wal_frames != sync.checkpointed_frames) return 15;
#endif
    if (pw_volume_close(volume) != PW_BUSY) return 13;
    REQUIRE(pw_read_view_close(volume, view));
    if (pw_read_view_close(volume, view) != PW_INVALID) return 14;
    REQUIRE(pw_volume_close(volume));
    REQUIRE(pw_volume_open((const uint8_t *)path, strlen(path), &volume));
    REQUIRE(pw_lookup(volume, 1, (const uint8_t *)"renamed.txt", 11, &found));
    if (node != found) return 7;
    REQUIRE(pw_lookup_entry(volume, 1, (const uint8_t *)"RENAMED.TXT", 11,
                           buffer, sizeof(buffer), &count, &found));
    if (count != 11 || memcmp(buffer, "renamed.txt", 11) != 0 || node != found) return 16;
    int64_t parent = 0;
    REQUIRE(pw_parent_directory(volume, 1, &parent));
    if (parent != 1) return 17;
    uint64_t cursor = 0;
    REQUIRE(pw_dir_begin(volume, 1, &cursor));
    REQUIRE(pw_dir_seek(volume, cursor, 1));
    if (pw_dir_next(volume, cursor, buffer, sizeof(buffer), &count, &found) != PW_END) return 18;
    REQUIRE(pw_dir_seek(volume, cursor, 0));
    REQUIRE(pw_dir_next(volume, cursor, buffer, sizeof(buffer), &count, &found));
    if (count != 11 || memcmp(buffer, "renamed.txt", 11) != 0) return 19;
    REQUIRE(pw_dir_close(volume, cursor));
    PwStat stat = {0};
    REQUIRE(pw_stat(volume, found, &stat));
    if (stat.kind != 0 || stat.size != 5 || stat.links != 1) return 8;
    REQUIRE(pw_remove(volume, 1, (const uint8_t *)"renamed.txt", 11, 0));
    REQUIRE(pw_volume_close(volume));
    char metadata_path[32768];
    int path_size = snprintf(metadata_path, sizeof(metadata_path), "%s-metadata", path);
    if (path_size < 0 || (size_t)path_size >= sizeof(metadata_path)) return 21;
    PwCreateAttributes attributes = {501, 20, 0750, 0};
    REQUIRE(pw_volume_create_with_metadata((const uint8_t *)metadata_path, strlen(metadata_path), 65536, &attributes, &volume));
    REQUIRE(pw_volume_identity(volume, &identity));
    if (identity.capabilities != 1 || (identity.uuid[6] >> 4) != 4 || (identity.uuid[8] >> 6) != 2) return 22;
    PwVolumeIdentity original_identity = identity;
    attributes.mode = 06751;
    PwFsKitCreated created_node;
    PwCreateState initial_state = {0};
    initial_state.valid = PW_CREATE_SIZE | PW_CREATE_CREATED;
    initial_state.size = 4;
    initial_state.created.seconds = -321;
    initial_state.created.nanoseconds = 111111111;
    REQUIRE(pw_fskit_node_create_with_state(volume, 1, (const uint8_t *)"tool", 4, 0, &attributes, &initial_state, &created_node));
    if (created_node.attributes.size != 4 || created_node.attributes.metadata.created.seconds != -321 || created_node.attributes.metadata.created.nanoseconds != 111111111) return 57;
    node = created_node.object;
    if (!created_node.file || created_node.attributes.file_id != (uint64_t)node + 1 || created_node.attributes.metadata.mode != 06751) return 55;
    if (pw_volume_close(volume) != PW_BUSY) return 56;
    REQUIRE(pw_file_close(volume, created_node.file));
    REQUIRE(pw_metadata_get(volume, node, &metadata));
    if (metadata.uid != 501 || metadata.gid != 20 || metadata.mode != 06751 || metadata.reserved != 0) return 23;
    PwMetadataUpdate update = {0};
    update.valid = PW_META_ACCESSED | PW_META_MODIFIED;
    update.accessed.seconds = -123;
    update.accessed.nanoseconds = 999999999;
    update.modified = update.accessed;
    REQUIRE(pw_metadata_set(volume, node, &update));
    update.valid = 64;
    if (pw_metadata_set(volume, node, &update) != PW_INVALID) return 24;
    REQUIRE(pw_file_open(volume, node, &handle));
    REQUIRE(pw_file_write(volume, handle, 0, (const uint8_t *)"code", 4, &count));
    if (pw_file_truncate_with_metadata(volume, handle, 2, NULL) != PW_INVALID) return 28;
    REQUIRE(pw_stat(volume, node, &stat));
    if (stat.size != 4) return 29;
    update.valid = PW_META_MODE | PW_META_MODIFIED;
    update.mode = 06751;
    update.modified.seconds = -456;
    update.modified.nanoseconds = 123456789;
    PwFsKitAttributes committed_attributes;
    REQUIRE(pw_fskit_file_set_attributes(volume, handle, node, 2, &update, &committed_attributes));
    if (committed_attributes.file_id != (uint64_t)node + 1 || committed_attributes.size != 2 ||
        committed_attributes.metadata_valid != 1 || committed_attributes.metadata.mode != 0751 ||
        committed_attributes.metadata.modified.seconds != -456 || committed_attributes.metadata.modified.nanoseconds != 123456789) return 53;
    REQUIRE(pw_file_close(volume, handle));
    REQUIRE(pw_volume_close(volume));
    REQUIRE(pw_volume_open((const uint8_t *)metadata_path, strlen(metadata_path), &volume));
    REQUIRE(pw_volume_identity(volume, &identity));
    if (memcmp(identity.uuid, original_identity.uuid, 16) != 0) return 25;
    REQUIRE(pw_metadata_get(volume, node, &metadata));
    if (metadata.mode != 0751 || metadata.accessed.seconds != -123 ||
        metadata.accessed.nanoseconds != 999999999 || metadata.modified.seconds != -456 ||
        metadata.modified.nanoseconds != 123456789) return 26;
    REQUIRE(pw_stat(volume, node, &stat));
    if (stat.size != 2) return 27;
    PwCredentials owner = {501, 55, NULL, 0, 0};
    PwCredentials other = {502, 55, NULL, 0, 0};
    int64_t protected_node = 99;
    if (pw_posix_create(volume, 1, (const uint8_t *)"absent", 6, 0, 0666, 0027, &other, &protected_node) != PW_DENIED || protected_node != 99) return 30;
    update.valid = PW_META_MODE;
    update.mode = 01777;
    REQUIRE(pw_fskit_metadata_set(volume, 1, &update, &committed_attributes));
    if (committed_attributes.file_id != 2 || committed_attributes.kind != 1 || committed_attributes.metadata.mode != 01777) return 54;
    REQUIRE(pw_posix_create(volume, 1, (const uint8_t *)"owned", 5, 0, 0666, 0027, &owner, &protected_node));
    REQUIRE(pw_metadata_get(volume, protected_node, &metadata));
    if (metadata.uid != 501 || metadata.gid != 55 || metadata.mode != 0640) return 31;
    REQUIRE(pw_posix_check_access(volume, 1, (const uint8_t *)"owned", 5, PW_ACCESS_READ, &other));
    if (pw_posix_check_access(volume, 1, (const uint8_t *)"owned", 5, PW_ACCESS_WRITE, &other) != PW_DENIED ||
        pw_posix_remove(volume, 1, (const uint8_t *)"owned", 5, 0, &other) != PW_DENIED) return 32;
    uint32_t maintenance = 99, protected_state = 99;
    REQUIRE(pw_health(volume, &maintenance, &protected_state));
    if (maintenance != 0 || protected_state != 0) return 33;
    REQUIRE(pw_posix_rename(volume, 1, (const uint8_t *)"owned", 5, 1, (const uint8_t *)"renamed", 7, 0, &owner));
    REQUIRE(pw_posix_remove(volume, 1, (const uint8_t *)"renamed", 7, 0, &owner));
    REQUIRE(pw_posix_create(volume, 1, (const uint8_t *)"cap", 3, 0, 0666, 0027, &owner, &protected_node));
    uint64_t reader = 99, writer = 0;
    if (pw_posix_file_open(volume, 1, (const uint8_t *)"cap", 3, PW_ACCESS_READ | PW_ACCESS_WRITE, &other, &reader) != PW_DENIED || reader != 99) return 34;
    REQUIRE(pw_posix_file_open(volume, 1, (const uint8_t *)"cap", 3, PW_ACCESS_READ, &other, &reader));
    REQUIRE(pw_posix_file_open(volume, 1, (const uint8_t *)"cap", 3, PW_ACCESS_READ | PW_ACCESS_WRITE, &owner, &writer));
    if (pw_volume_close(volume) != PW_BUSY) return 35;
    REQUIRE(pw_posix_file_write(volume, writer, 0, (const uint8_t *)"abcdef", 6, &count));
    PwFileInfo request_info;
    PwWriteResult request_result;
    REQUIRE(pw_posix_file_info(volume, reader, &request_info));
    if (request_info.object != protected_node || request_info.stat.size != 6) return 44;
    REQUIRE(pw_posix_file_write_request(volume, writer, UINT64_MAX, (const uint8_t *)"gh", 2, PW_WRITE_APPEND, &request_result));
    if (request_result.offset != 6 || request_result.written != 2 || request_result.info.stat.size != 8) return 45;
    REQUIRE(pw_posix_file_write_request(volume, writer, 7, (const uint8_t *)"XYZ", 3, PW_WRITE_CONSTRAINED, &request_result));
    if (request_result.written != 1 || request_result.info.stat.size != 8) return 46;
    REQUIRE(pw_posix_file_truncate_info(volume, writer, 6, &request_info));
    if (request_info.stat.size != 6 || request_info.metadata.uid != owner.uid) return 47;
    REQUIRE(pw_posix_file_write_request(volume, writer, UINT64_MAX, NULL, 0, PW_WRITE_APPEND, &request_result));
    if (request_result.written != 0 || request_result.offset != 6 || request_result.info.stat.generation != request_info.stat.generation) return 48;
    PwWindowsFileInfo windows_info;
    PwWindowsWriteResult windows_write;
    REQUIRE(pw_windows_file_info(volume, reader, &windows_info));
    if (windows_info.index_number != (uint64_t)protected_node || windows_info.file_size != 6 || windows_info.allocation_size != 4096) return 50;
    REQUIRE(pw_windows_file_write(volume, writer, UINT64_MAX, (const uint8_t *)"!", 1, PW_WRITE_APPEND, &windows_write));
    if (windows_write.offset != 6 || windows_write.written != 1 || windows_write.info.file_size != 7) return 51;
    REQUIRE(pw_windows_file_set_size(volume, writer, 6, &windows_info));
    if (windows_info.file_size != 6 || pw_windows_file_write(volume, writer, 0, (const uint8_t *)"bad", 3, 0, NULL) != PW_INVALID) return 52;
    PwWindowsBasicInfo basic = {UINT32_MAX, 0, 1, UINT64_C(116444736000000007), UINT64_C(116444735999999999), 0};
    REQUIRE(pw_windows_file_set_basic_info(volume, reader, &owner, &basic, &windows_info));
    if (windows_info.creation_time != 1 || windows_info.last_access_time != UINT64_C(116444736000000007) ||
        windows_info.last_write_time != UINT64_C(116444735999999999) || windows_info.file_size != 6) return 58;
    REQUIRE(pw_metadata_get(volume, protected_node, &metadata));
    if (metadata.created.seconds != -INT64_C(11644473600) || metadata.created.nanoseconds != 100 ||
        metadata.accessed.seconds != 0 || metadata.accessed.nanoseconds != 700 ||
        metadata.modified.seconds != -1 || metadata.modified.nanoseconds != 999999900) return 59;
    if (pw_windows_file_set_basic_info(volume, writer, &other, &basic, &windows_info) != PW_DENIED) return 60;
    basic.file_attributes = 0x80; /* No persistent DOS flags; cannot accept NORMAL as a fake no-op. */
    if (pw_windows_file_set_basic_info(volume, writer, &owner, &basic, &windows_info) != PW_UNSUPPORTED ||
        windows_info.creation_time != 1) return 61;
    update.valid = PW_META_MODIFIED;
    update.modified.seconds = -123;
    update.modified.nanoseconds = 987654321;
    REQUIRE(pw_posix_file_set_metadata_info(volume, reader, &owner, &update, &request_info));
    if (request_info.object != protected_node || request_info.stat.size != 6 ||
        request_info.metadata.modified.nanoseconds != 987654321) return 62;
    if (pw_posix_file_write_request(volume, writer, 0, (const uint8_t *)"bad", 3, 0, NULL) != PW_INVALID ||
        pw_posix_file_write_request(volume, reader, 0, NULL, 0, PW_WRITE_APPEND, &request_result) != PW_DENIED) return 49;
    count = 99;
    if (pw_posix_file_write(volume, reader, 0, (const uint8_t *)"bad", 3, &count) != PW_DENIED || count != 99 ||
        pw_file_read(volume, reader, 0, buffer, sizeof(buffer), &count) != PW_INVALID || count != 99) return 36;
    update.valid = PW_META_MODE;
    update.mode = 0;
    REQUIRE(pw_posix_metadata_set(volume, 1, (const uint8_t *)"cap", 3, &owner, &update));
    REQUIRE(pw_posix_file_read(volume, reader, 0, buffer, sizeof(buffer), &count));
    if (count != 6 || memcmp(buffer, "abcdef", 6) != 0) return 37;
    uint64_t policy_view = 99;
    REQUIRE(pw_posix_read_view_begin(volume, reader, 1, 3, &policy_view));
    update.valid = PW_META_MODE | PW_META_MODIFIED;
    update.mode = 06751;
    update.modified.seconds = -789;
    update.modified.nanoseconds = 987654321;
    if (pw_posix_file_truncate_with_metadata(volume, writer, 0, &other, &update) != PW_DENIED) return 38;
    REQUIRE(pw_posix_file_truncate_with_metadata(volume, writer, 3, &owner, &update));
    REQUIRE(pw_metadata_get(volume, protected_node, &metadata));
    if (metadata.mode != 0751 || metadata.modified.seconds != -789 || metadata.modified.nanoseconds != 987654321) return 39;
    REQUIRE(pw_posix_remove(volume, 1, (const uint8_t *)"cap", 3, 0, &owner));
    REQUIRE(pw_posix_file_read(volume, reader, 0, buffer, sizeof(buffer), &count));
    if (count != 3 || memcmp(buffer, "abc", 3) != 0) return 40;
    REQUIRE(pw_posix_file_close(volume, reader));
    if (pw_posix_file_close(volume, reader) != PW_INVALID) return 41;
    REQUIRE(pw_posix_file_close(volume, writer));
    if (pw_volume_close(volume) != PW_BUSY) return 42;
    count = 99;
    if (pw_posix_read_view_read(volume, policy_view, 0, buffer, 1, &count) != PW_INVALID || count != 99 ||
        pw_read_view_read(volume, policy_view, 1, buffer, 3, &count) != PW_INVALID || count != 99) return 43;
    REQUIRE(pw_posix_read_view_read(volume, policy_view, 1, buffer, 3, &count));
    if (count != 3 || memcmp(buffer, "bcd", 3) != 0) return 44;
    REQUIRE(pw_posix_read_view_close(volume, policy_view));
    if (pw_posix_read_view_close(volume, policy_view) != PW_INVALID) return 45;
    REQUIRE(pw_volume_close(volume));
    puts("NATIVE_C_ABI_OK");
    return 0;
}
