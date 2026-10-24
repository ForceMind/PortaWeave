/*
 * M0-G WinFsp 2.1 callback CONTRACT probe: no driver installed and no mount.
 * All unimplemented normal filesystem operations are denied, never faked.
 * This executable exercises callbacks directly with a mock WinFsp context.
 * WinFsp's real FSP dispatcher/mount remains NOT_RUN/BLOCKED.
 */
#include <winfsp/winfsp.h>
#include "portaweave_native.h"
#include <stdio.h>
#include <limits.h>
#include <string.h>

typedef struct PW_PROBE_VOLUME {
    uint64_t volume;
} PW_PROBE_VOLUME;
typedef struct PW_PROBE_FILE {
    uint64_t file;
} PW_PROBE_FILE;

static NTSTATUS PwNtStatus(int32_t result)
{
    switch (result) {
    case PW_OK: return STATUS_SUCCESS;
    case PW_NOT_FOUND: return STATUS_OBJECT_NAME_NOT_FOUND;
    case PW_EXISTS: return STATUS_OBJECT_NAME_COLLISION;
    case PW_NOT_DIR: return STATUS_NOT_A_DIRECTORY;
    case PW_IS_DIR: return STATUS_FILE_IS_A_DIRECTORY;
    case PW_NOT_EMPTY: return STATUS_DIRECTORY_NOT_EMPTY;
    case PW_BUSY: return STATUS_SHARING_VIOLATION;
    case PW_NO_SPACE: return STATUS_DISK_FULL;
    case PW_QUOTA: return STATUS_QUOTA_EXCEEDED;
    case PW_READ_ONLY: return STATUS_MEDIA_WRITE_PROTECTED;
    case PW_UNSUPPORTED: return STATUS_NOT_SUPPORTED;
    case PW_CORRUPT: return STATUS_DATA_ERROR;
    case PW_IO: return STATUS_IO_DEVICE_ERROR;
    case PW_RESOURCE: return STATUS_INSUFFICIENT_RESOURCES;
    case PW_BUFFER_TOO_SMALL: return STATUS_BUFFER_TOO_SMALL;
    default: return STATUS_INVALID_PARAMETER;
    }
}

/* Actual callback signature from upstream pinned WinFsp v2.1 headers. */
static NTSTATUS PwGetVolumeInfo(FSP_FILE_SYSTEM *fs, FSP_FSCTL_VOLUME_INFO *out)
{
    PW_PROBE_VOLUME *ctx;
    PwSpace space = {0};
    NTSTATUS status;
    const WCHAR label[] = L"PortaWeave";
    if (NULL == fs || NULL == fs->UserContext || NULL == out)
        return STATUS_INVALID_PARAMETER;
    ctx = (PW_PROBE_VOLUME *)fs->UserContext;
    status = PwNtStatus(pw_volume_space(ctx->volume, &space));
    if (!NT_SUCCESS(status))
        return status;
    ZeroMemory(out, sizeof(*out));
    out->TotalSize = space.logical_quota_bytes;
    out->FreeSize = space.apparent_free_bytes;
    out->VolumeLabelLength = (UINT16)(sizeof(label) - sizeof(WCHAR));
    CopyMemory(out->VolumeLabel, label, out->VolumeLabelLength);
    return STATUS_SUCCESS;
}

/* Until effective ACL/storage and token checks exist, NEVER grant access. */
static NTSTATUS PwGetSecurityByName(
    FSP_FILE_SYSTEM *fs, PWSTR path, PUINT32 attributes,
    PSECURITY_DESCRIPTOR descriptor, SIZE_T *required)
{
    (void)fs;
    (void)path;
    (void)attributes;
    (void)descriptor;
    (void)required;
    return STATUS_ACCESS_DENIED;
}
static NTSTATUS PwOpen(
    FSP_FILE_SYSTEM *fs, PWSTR path, UINT32 opts, UINT32 access,
    PVOID *out_context, FSP_FSCTL_FILE_INFO *info)
{
    (void)fs;
    (void)path;
    (void)opts;
    (void)access;
    (void)out_context;
    (void)info;
    return STATUS_ACCESS_DENIED;
}

/* A synthetic file context from the test; user opens remain forbidden. */
static NTSTATUS PwRead(
    FSP_FILE_SYSTEM *fs, PVOID context, PVOID buffer, UINT64 offset,
    ULONG length, PULONG transferred)
{
    PW_PROBE_VOLUME *volume;
    PW_PROBE_FILE *file;
    size_t done = 0;
    NTSTATUS status;
    if (NULL == fs || NULL == fs->UserContext || NULL == context ||
        NULL == transferred || (length && NULL == buffer))
        return STATUS_INVALID_PARAMETER;
    volume = (PW_PROBE_VOLUME *)fs->UserContext;
    file = (PW_PROBE_FILE *)context;
    status = PwNtStatus(pw_file_read(
        volume->volume, file->file, offset, (uint8_t *)buffer, length, &done));
    if (!NT_SUCCESS(status))
        return status;
    if (done > ULONG_MAX)
        return STATUS_INVALID_PARAMETER;
    *transferred = (ULONG)done;
    return STATUS_SUCCESS;
}
static VOID PwClose(FSP_FILE_SYSTEM *fs, PVOID context)
{
    PW_PROBE_VOLUME *volume;
    PW_PROBE_FILE *file;
    if (!fs || !fs->UserContext || !context)
        return;
    volume = (PW_PROBE_VOLUME *)fs->UserContext;
    file = (PW_PROBE_FILE *)context;
    (void)pw_file_close(volume->volume, file->file);
}
/* A SQLite WAL checkpoint cannot satisfy filesystem Flush! Fail closed. */
static NTSTATUS PwFlush(
    FSP_FILE_SYSTEM *fs, PVOID context, FSP_FSCTL_FILE_INFO *info)
{
    (void)fs;
    (void)context;
    (void)info;
    return STATUS_NOT_SUPPORTED;
}
static VOID PwInterfaceInit(FSP_FILE_SYSTEM_INTERFACE *callbacks)
{
    ZeroMemory(callbacks, sizeof(*callbacks));
    callbacks->GetVolumeInfo = PwGetVolumeInfo;
    callbacks->GetSecurityByName = PwGetSecurityByName;
    callbacks->Open = PwOpen;
    callbacks->Read = PwRead;
    callbacks->Close = PwClose;
    callbacks->Flush = PwFlush;
    /* Create, Write, Rename, Directory, ACL, mmap etc. remain unset. */
}
#define REQUIRE(condition) do { if (!(condition)) { \
    fprintf(stderr, "check failed: %s at line %d\n", #condition, __LINE__); \
    return 1; \
} } while (0)
int main(int argc, char **argv)
{
    FSP_FILE_SYSTEM_INTERFACE callbacks;
    FSP_FILE_SYSTEM fs = {0};
    FSP_FSCTL_VOLUME_INFO info = {0};
    FSP_FSCTL_FILE_INFO fileInfo = {0};
    PW_PROBE_VOLUME context = {0};
    PW_PROBE_FILE file = {0};
    uint64_t volume = 0;
    int64_t node = 0;
    size_t count = 0;
    ULONG resultCount = 0;
    uint8_t output[64] = {0};
    if (argc != 2)
        return 2;
    REQUIRE(PW_OK == pw_volume_create(
        (const uint8_t *)argv[1], strlen(argv[1]), 65536, &volume));
    context.volume = volume;
    fs.UserContext = &context;
    PwInterfaceInit(&callbacks);
    REQUIRE(callbacks.GetVolumeInfo == PwGetVolumeInfo);
    REQUIRE(callbacks.Open == PwOpen);
    REQUIRE(callbacks.Flush == PwFlush);
    REQUIRE(callbacks.Write == NULL);
    REQUIRE(STATUS_SUCCESS == callbacks.GetVolumeInfo(&fs, &info));
    REQUIRE(info.TotalSize == 65536 && info.FreeSize <= info.TotalSize);
    REQUIRE(STATUS_ACCESS_DENIED == callbacks.GetSecurityByName(
        &fs, L"\\", NULL, NULL, NULL));
    REQUIRE(STATUS_ACCESS_DENIED == callbacks.Open(
        &fs, L"\\", 0, 0, NULL, &fileInfo));
    REQUIRE(STATUS_NOT_SUPPORTED == callbacks.Flush(&fs, NULL, NULL));
    REQUIRE(PW_OK == pw_file_create(volume, 1, (const uint8_t *)"probe.txt", 9, &node));
    REQUIRE(PW_OK == pw_file_open(volume, node, &file.file));
    REQUIRE(PW_OK == pw_file_write(
        volume, file.file, 0, (const uint8_t *)"payload", 7, &count));
    REQUIRE(count == 7);
    REQUIRE(STATUS_SUCCESS == callbacks.Read(
        &fs, &file, output, 0, sizeof(output), &resultCount));
    REQUIRE(resultCount == 7 && 0 == memcmp(output, "payload", 7));
    callbacks.Close(&fs, &file);
    REQUIRE(PW_OK == pw_volume_close(volume));
    puts("WINFSP_CALLBACK_CONTRACT_OK (no mount, no driver)");
    return 0;
}
