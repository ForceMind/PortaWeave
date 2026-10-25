/*
 * M0-G WinFsp 2.1 callback CONTRACT probe: no driver installed and no mount.
 * Granted v2 regular-file I/O is exercised; OS opens/security remain denied.
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
    uint64_t file; /* separately typed granted policy token, not raw admin token */
    int32_t close_status;
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
    case PW_DENIED: return STATUS_ACCESS_DENIED;
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

/* Test-only grant context; Open/GetSecurityByName never manufacture one. */
static VOID PwCopyFileInfo(const PwWindowsFileInfo *from, FSP_FSCTL_FILE_INFO *out)
{
    ZeroMemory(out, sizeof(*out));
    out->FileAttributes = FILE_ATTRIBUTE_NORMAL; /* not a POSIX mode/ACL mapping */
    out->AllocationSize = from->allocation_size;
    out->FileSize = from->file_size;
    out->CreationTime = from->creation_time;
    out->LastAccessTime = from->last_access_time;
    out->LastWriteTime = from->last_write_time;
    out->ChangeTime = from->change_time;
    out->IndexNumber = from->index_number;
    /* Pinned WinFsp v2.1 requires HardLinks=0 (interface unimplemented). */
}
static NTSTATUS PwGetFileInfo(FSP_FILE_SYSTEM *fs, PVOID context, FSP_FSCTL_FILE_INFO *out)
{
    PW_PROBE_VOLUME *volume;
    PW_PROBE_FILE *file;
    PwWindowsFileInfo info;
    NTSTATUS status;
    if (!fs || !fs->UserContext || !context || !out)
        return STATUS_INVALID_PARAMETER;
    volume = (PW_PROBE_VOLUME *)fs->UserContext;
    file = (PW_PROBE_FILE *)context;
    status = PwNtStatus(pw_windows_file_info(volume->volume, file->file, &info));
    if (!NT_SUCCESS(status)) return status;
    PwCopyFileInfo(&info, out);
    return STATUS_SUCCESS;
}
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
    status = PwNtStatus(pw_posix_file_read(
        volume->volume, file->file, offset, (uint8_t *)buffer, length, &done));
    if (!NT_SUCCESS(status))
        return status;
    if (done > ULONG_MAX)
        return STATUS_INVALID_PARAMETER;
    *transferred = (ULONG)done;
    return length && !done ? STATUS_END_OF_FILE : STATUS_SUCCESS;
}
static NTSTATUS PwWrite(
    FSP_FILE_SYSTEM *fs, PVOID context, PVOID buffer, UINT64 offset,
    ULONG length, BOOLEAN append, BOOLEAN constrained, PULONG transferred,
    FSP_FSCTL_FILE_INFO *out)
{
    PW_PROBE_VOLUME *volume;
    PW_PROBE_FILE *file;
    PwWindowsWriteResult result;
    uint32_t flags = 0;
    NTSTATUS status;
    if (!fs || !fs->UserContext || !context || !transferred || !out ||
        (length && !buffer) || append > 1 || constrained > 1)
        return STATUS_INVALID_PARAMETER;
    volume = (PW_PROBE_VOLUME *)fs->UserContext;
    file = (PW_PROBE_FILE *)context;
    if (append) flags |= PW_WRITE_APPEND;
    if (constrained) flags |= PW_WRITE_CONSTRAINED;
    status = PwNtStatus(pw_windows_file_write(volume->volume, file->file,
        offset, (const uint8_t *)buffer, length, flags, &result));
    if (!NT_SUCCESS(status)) return status;
    /* The ABI caps requests at 16 MiB. Nothing fallible follows commit. */
    *transferred = (ULONG)result.written;
    PwCopyFileInfo(&result.info, out);
    return STATUS_SUCCESS;
}
static NTSTATUS PwSetFileSize(
    FSP_FILE_SYSTEM *fs, PVOID context, UINT64 size, BOOLEAN allocation,
    FSP_FSCTL_FILE_INFO *out)
{
    PW_PROBE_VOLUME *volume;
    PW_PROBE_FILE *file;
    PwWindowsFileInfo info;
    NTSTATUS status;
    if (!fs || !fs->UserContext || !context || !out || allocation > 1)
        return STATUS_INVALID_PARAMETER;
    /* No physical reservation/preallocation exists. Never fake it or truncate
     * while pretending to accept an unsupported allocation request. */
    if (allocation) return STATUS_NOT_SUPPORTED;
    volume = (PW_PROBE_VOLUME *)fs->UserContext;
    file = (PW_PROBE_FILE *)context;
    status = PwNtStatus(pw_windows_file_set_size(volume->volume, file->file, size, &info));
    if (!NT_SUCCESS(status)) return status;
    PwCopyFileInfo(&info, out);
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
    file->close_status = pw_posix_file_close(volume->volume, file->file);
    if (file->close_status == PW_OK) file->file = 0;
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
    callbacks->GetFileInfo = PwGetFileInfo;
    callbacks->Write = PwWrite;
    callbacks->SetFileSize = PwSetFileSize;
    callbacks->Close = PwClose;
    callbacks->Flush = PwFlush;
    /* Create, Rename, Directory, ACL, mmap etc. remain unset. */
}
#ifndef PW_WINFSP_NO_TEST
#define REQUIRE(condition) do { if (!(condition)) { \
    fprintf(stderr, "check failed: %s at line %d\n", #condition, __LINE__); \
    return 1; \
} } while (0)
int main(int argc, char **argv)
{
    FSP_FILE_SYSTEM_INTERFACE callbacks;
    FSP_FILE_SYSTEM fs = {0};
    FSP_FSCTL_VOLUME_INFO info = {0};
    FSP_FSCTL_FILE_INFO fileInfo = {0}, oldInfo;
    PW_PROBE_VOLUME context = {0};
    PW_PROBE_FILE file = {0}, reader = {0}, raw = {0};
    PwCreateAttributes attrs = {100, 20, 0777, 0};
    PwCredentials caller = {100, 20, NULL, 0, 0};
    PwMetadataUpdate update = {0};
    uint64_t volume = 0;
    int64_t node = 0;
    ULONG resultCount = 0;
    uint8_t output[64] = {0};
    if (argc != 2) return 2;
    REQUIRE(PwNtStatus(PW_DENIED) == STATUS_ACCESS_DENIED);
    REQUIRE(PwNtStatus(PW_READ_ONLY) == STATUS_MEDIA_WRITE_PROTECTED);
    REQUIRE(PW_OK == pw_volume_create_with_metadata(
        (const uint8_t *)argv[1], strlen(argv[1]), 65536, &attrs, &volume));
    context.volume = volume;
    fs.UserContext = &context;
    PwInterfaceInit(&callbacks);
    REQUIRE(callbacks.GetFileInfo == PwGetFileInfo && callbacks.Write == PwWrite &&
        callbacks.SetFileSize == PwSetFileSize && callbacks.Create == NULL);
    REQUIRE(STATUS_SUCCESS == callbacks.GetVolumeInfo(&fs, &info));
    REQUIRE(info.TotalSize == 65536 && info.FreeSize <= info.TotalSize);
    REQUIRE(STATUS_ACCESS_DENIED == callbacks.GetSecurityByName(&fs, L"\\", NULL, NULL, NULL));
    REQUIRE(STATUS_ACCESS_DENIED == callbacks.Open(&fs, L"\\", 0, 0, NULL, &fileInfo));
    REQUIRE(STATUS_NOT_SUPPORTED == callbacks.Flush(&fs, NULL, NULL));
    REQUIRE(PW_OK == pw_posix_create(volume, 1, (const uint8_t *)"probe.txt", 9, 0, 0600, 0, &caller, &node));
    REQUIRE(PW_OK == pw_posix_file_open(volume, 1, (const uint8_t *)"probe.txt", 9, 6, &caller, &file.file));
    REQUIRE(PW_OK == pw_posix_file_open(volume, 1, (const uint8_t *)"probe.txt", 9, 4, &caller, &reader.file));
    REQUIRE(STATUS_SUCCESS == callbacks.Write(&fs, &file, "payload", 0, 7, FALSE, FALSE, &resultCount, &fileInfo));
    REQUIRE(resultCount == 7 && fileInfo.FileSize == 7 && fileInfo.AllocationSize == 4096 &&
        fileInfo.IndexNumber == (UINT64)node && fileInfo.HardLinks == 0 &&
        fileInfo.FileAttributes == FILE_ATTRIBUTE_NORMAL && fileInfo.LastWriteTime != 0);
    REQUIRE(STATUS_SUCCESS == callbacks.Read(&fs, &reader, output, 0, sizeof(output), &resultCount));
    REQUIRE(resultCount == 7 && 0 == memcmp(output, "payload", 7));
    REQUIRE(STATUS_SUCCESS == callbacks.GetFileInfo(&fs, &file, &oldInfo));
    REQUIRE(0 == memcmp(&fileInfo, &oldInfo, sizeof(fileInfo)));
    REQUIRE(STATUS_SUCCESS == callbacks.Write(&fs, &file, "!", UINT64_MAX, 1, TRUE, FALSE, &resultCount, &fileInfo));
    REQUIRE(resultCount == 1 && fileInfo.FileSize == 8);
    REQUIRE(STATUS_SUCCESS == callbacks.Write(&fs, &file, "XYZ", 7, 3, FALSE, TRUE, &resultCount, &fileInfo));
    REQUIRE(resultCount == 1 && fileInfo.FileSize == 8);
    oldInfo = fileInfo;
    REQUIRE(STATUS_SUCCESS == callbacks.Write(&fs, &file, "x", UINT64_MAX, 1, TRUE, TRUE, &resultCount, &fileInfo));
    REQUIRE(resultCount == 0 && 0 == memcmp(&fileInfo, &oldInfo, sizeof(fileInfo)));
    resultCount = 77;
    REQUIRE(STATUS_ACCESS_DENIED == callbacks.Write(&fs, &reader, NULL, 0, 0, FALSE, FALSE, &resultCount, &fileInfo));
    REQUIRE(resultCount == 77 && 0 == memcmp(&fileInfo, &oldInfo, sizeof(fileInfo)));
    REQUIRE(STATUS_ACCESS_DENIED == callbacks.SetFileSize(&fs, &reader, 0, FALSE, &fileInfo));
    REQUIRE(STATUS_NOT_SUPPORTED == callbacks.SetFileSize(&fs, &file, 0, TRUE, &fileInfo));
    REQUIRE(0 == memcmp(&fileInfo, &oldInfo, sizeof(fileInfo)));
    REQUIRE(STATUS_INVALID_PARAMETER == callbacks.Write(&fs, &file, "bad", 0, 3, FALSE, FALSE, NULL, &fileInfo));
    REQUIRE(STATUS_INVALID_PARAMETER == callbacks.Write(&fs, &file, "bad", 0, 3, FALSE, FALSE, &resultCount, NULL));
    REQUIRE(STATUS_SUCCESS == callbacks.SetFileSize(&fs, &file, 3, FALSE, &fileInfo));
    REQUIRE(fileInfo.FileSize == 3);
    REQUIRE(STATUS_SUCCESS == callbacks.SetFileSize(&fs, &file, 8, FALSE, &fileInfo));
    REQUIRE(STATUS_SUCCESS == callbacks.Read(&fs, &reader, output, 0, sizeof(output), &resultCount));
    REQUIRE(resultCount == 8 && 0 == memcmp(output, "pay\0\0\0\0\0", 8));
    REQUIRE(STATUS_END_OF_FILE == callbacks.Read(&fs, &reader, output, 8, 1, &resultCount));
    REQUIRE(resultCount == 0);
    REQUIRE(STATUS_SUCCESS == callbacks.Read(&fs, &reader, NULL, 8, 0, &resultCount));
    /* A raw administrative token must not bypass the granted context table. */
    REQUIRE(PW_OK == pw_file_open(volume, node, &raw.file));
    REQUIRE(STATUS_INVALID_PARAMETER == callbacks.Read(&fs, &raw, output, 0, 1, &resultCount));
    REQUIRE(STATUS_INVALID_PARAMETER == callbacks.GetFileInfo(&fs, &raw, &fileInfo));
    REQUIRE(PW_OK == pw_file_close(volume, raw.file));
    /* Bad projection rejects before commit: no false error after a successful write. */
    update.valid = PW_META_CREATED;
    update.created.seconds = -INT64_C(11644473601);
    REQUIRE(PW_OK == pw_metadata_set(volume, node, &update));
    REQUIRE(STATUS_NOT_SUPPORTED == callbacks.Write(&fs, &file, "bad", UINT64_MAX, 3, TRUE, FALSE, &resultCount, &fileInfo));
    REQUIRE(STATUS_SUCCESS == callbacks.Read(&fs, &reader, output, 0, sizeof(output), &resultCount));
    REQUIRE(resultCount == 8 && 0 == memcmp(output, "pay\0\0\0\0\0", 8));
    update.created.seconds = 0;
    REQUIRE(PW_OK == pw_metadata_set(volume, node, &update));
    /* Still-open inode data and identity survive final unlink. */
    REQUIRE(PW_OK == pw_posix_remove(volume, 1, (const uint8_t *)"probe.txt", 9, 0, &caller));
    REQUIRE(STATUS_SUCCESS == callbacks.GetFileInfo(&fs, &file, &fileInfo));
    REQUIRE(fileInfo.IndexNumber == (UINT64)node);
    callbacks.Close(&fs, &reader); callbacks.Close(&fs, &file);
    REQUIRE(reader.file == 0 && file.file == 0 && reader.close_status == PW_OK && file.close_status == PW_OK);
    REQUIRE(STATUS_INVALID_PARAMETER == callbacks.GetFileInfo(&fs, &file, &fileInfo));
    REQUIRE(PW_OK == pw_volume_close(volume));
    puts("WINFSP_GRANTED_IO_CONTRACT_OK (regular-file grants; no ACL/Open/mount certification)");
    puts("WINFSP_CALLBACK_CONTRACT_OK (no mount, no driver)");
    return 0;
}

#endif /* PW_WINFSP_NO_TEST */
