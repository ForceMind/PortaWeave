/*
 * M0-G WinFsp 2.1 callback CONTRACT probe: no driver installed and no mount.
 * Granted v2/v3 regular-file and explicit v3 directory I/O is exercised;
 * OS opens/security remain denied.
 * This executable exercises callbacks directly with a mock WinFsp context.
 * WinFsp's real FSP dispatcher/mount remains NOT_RUN/BLOCKED.
 */
#include <winfsp/winfsp.h>
#include "portaweave_native.h"
#include <stdio.h>
#include <limits.h>
#include <string.h>
#include <stddef.h>
#include <wchar.h>
#include <stdlib.h>

/* Native bridge emits the pinned SDK's wire layout, not a Rust C struct.
 * SDK drift must fail compilation instead of silently corrupting directory I/O. */
FSP_FSCTL_STATIC_ASSERT(sizeof(WCHAR) == 2, "UTF-16 units must be 2 bytes");
FSP_FSCTL_STATIC_ASSERT(sizeof(FSP_FSCTL_DIR_INFO) == 104, "directory header changed");
FSP_FSCTL_STATIC_ASSERT(offsetof(FSP_FSCTL_DIR_INFO, FileInfo) == 8, "file info offset changed");
FSP_FSCTL_STATIC_ASSERT(offsetof(FSP_FSCTL_DIR_INFO, NextOffset) == 80, "cookie offset changed");
FSP_FSCTL_STATIC_ASSERT(offsetof(FSP_FSCTL_DIR_INFO, FileNameBuf) == 104, "name offset changed");
FSP_FSCTL_STATIC_ASSERT(FSP_FSCTL_DEFAULT_ALIGNMENT == 8, "directory alignment changed");

typedef struct PW_PROBE_VOLUME {
    uint64_t volume;
} PW_PROBE_VOLUME;
typedef struct PW_PROBE_FILE {
    uint64_t file; /* separately typed granted policy token, not raw admin token */
    uint64_t directory; /* typed immutable directory snapshot; never a live OS grant */
    uint64_t directory_grant; /* separately typed v3 directory inode, lab policy only */
    int32_t close_status;
    /* Test-only, lifetime-valid trusted claims. NULL denies metadata mutation
     * and fresh linked-directory observation. An admitted v3 directory grant
     * supplies its own info authority, never metadata mutation authority.
     * Open never manufactures this context. Future native ACL authorization
     * must not infer these claims from daemon UID or untrusted request data. */
    const PwCredentials *metadata_caller;
} PW_PROBE_FILE;
static BOOL PwDirectoryContext(const PW_PROBE_FILE *file)
{
    return file->directory || file->directory_grant;
}

static VOID PwVolumeParamsInit(FSP_FSCTL_VOLUME_PARAMS *options)
{
    const WCHAR name[] = L"PortaWeaveLab";
    ZeroMemory(options, sizeof(*options));
    options->SectorSize = 4096;
    options->SectorsPerAllocationUnit = 1;
    options->FileInfoTimeout = 1000;
    options->CasePreservedNames = 1;
    options->UnicodeOnDisk = 1;
    options->DirectoryMarkerAsNextOffset = 1;
    /* PersistentAcls/HardLinks/security remain closed. Pattern filtering is FSD-owned. */
    CopyMemory(options->FileSystemName, name, sizeof(name));
}

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
    case PW_INVALID:
    case PW_BAD_COOKIE: return STATUS_INVALID_PARAMETER;
    /* A checked name changed: obtain a fresh observation, never retry a
     * destructive request blindly with the same stale expected identity. */
    case PW_STALE: return STATUS_RETRY;
    case PW_LOCK_CONFLICT: return STATUS_FILE_LOCK_CONFLICT;
    case PW_INTERNAL: return STATUS_INTERNAL_ERROR;
    /* PW_END belongs to cursor iteration, not this operation-result adapter.
     * Unexpected positive/negative codes must not look like success or an
     * invalid user argument. Preserve an internal/protocol failure. */
    default: return STATUS_INTERNAL_ERROR;
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
    if (file->directory_grant) {
        if (file->file) return STATUS_INVALID_PARAMETER;
        status = PwNtStatus(pw_windows_directory_handle_info(volume->volume, file->directory_grant, &info));
    } else if (file->directory) {
        if (file->file) return STATUS_INVALID_PARAMETER;
        if (!file->metadata_caller) return STATUS_ACCESS_DENIED;
        status = PwNtStatus(pw_windows_directory_info(volume->volume, file->directory,
            file->metadata_caller, &info));
    } else {
        status = PwNtStatus(pw_windows_file_info(volume->volume, file->file, &info));
    }
    if (!NT_SUCCESS(status)) return status;
    PwCopyFileInfo(&info, out);
    if (PwDirectoryContext(file)) out->FileAttributes = FILE_ATTRIBUTE_DIRECTORY;
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
    if (PwDirectoryContext(file)) return file->file ? STATUS_INVALID_PARAMETER : STATUS_FILE_IS_A_DIRECTORY;
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
    if (PwDirectoryContext(file)) return file->file ? STATUS_INVALID_PARAMETER : STATUS_FILE_IS_A_DIRECTORY;
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
    if (PwDirectoryContext(file)) return file->file ? STATUS_INVALID_PARAMETER : STATUS_FILE_IS_A_DIRECTORY;
    status = PwNtStatus(pw_windows_file_set_size(volume->volume, file->file, size, &info));
    if (!NT_SUCCESS(status)) return status;
    PwCopyFileInfo(&info, out);
    return STATUS_SUCCESS;
}
static NTSTATUS PwSetBasicInfo(
    FSP_FILE_SYSTEM *fs, PVOID context, UINT32 attributes,
    UINT64 created, UINT64 accessed, UINT64 modified, UINT64 changed,
    FSP_FSCTL_FILE_INFO *out)
{
    PW_PROBE_VOLUME *volume;
    PW_PROBE_FILE *file;
    PwWindowsBasicInfo request;
    PwWindowsFileInfo info;
    NTSTATUS status;
    if (!fs || !fs->UserContext || !context || !out)
        return STATUS_INVALID_PARAMETER;
    volume = (PW_PROBE_VOLUME *)fs->UserContext;
    file = (PW_PROBE_FILE *)context;
    if (file->file && PwDirectoryContext(file)) return STATUS_INVALID_PARAMETER;
    if (file->directory && !file->directory_grant) return STATUS_NOT_SUPPORTED;
    if (!file->metadata_caller) return STATUS_ACCESS_DENIED;
    ZeroMemory(&request, sizeof(request));
    request.file_attributes = attributes;
    request.creation_time = created;
    request.last_access_time = accessed;
    request.last_write_time = modified;
    request.change_time = changed;
    status = PwNtStatus(file->directory_grant ?
        pw_windows_directory_handle_set_basic_info(volume->volume, file->directory_grant, file->metadata_caller, &request, &info) :
        pw_windows_file_set_basic_info(volume->volume, file->file, file->metadata_caller, &request, &info));
    if (!NT_SUCCESS(status)) return status;
    /* Projection and authorization completed before commit. Copy cannot fail. */
    PwCopyFileInfo(&info, out);
    if (file->directory_grant) out->FileAttributes = FILE_ATTRIBUTE_DIRECTORY;
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
    if (file->file && PwDirectoryContext(file)) { file->close_status = PW_INVALID; return; }
    if (PwDirectoryContext(file)) {
        if (file->directory) {
            file->close_status = pw_windows_directory_close(volume->volume, file->directory);
            if (file->close_status != PW_OK) return;
            file->directory = 0;
        }
        if (file->directory_grant) {
            file->close_status = pw_posix_directory_close(volume->volume, file->directory_grant);
            if (file->close_status == PW_OK) file->directory_grant = 0;
        }
    } else {
        file->close_status = pw_posix_file_close(volume->volume, file->file);
        if (file->close_status == PW_OK) file->file = 0;
    }
}
static NTSTATUS PwReadDirectory(
    FSP_FILE_SYSTEM *fs, PVOID context, PWSTR pattern, PWSTR marker,
    PVOID buffer, ULONG length, PULONG transferred)
{
    PW_PROBE_VOLUME *volume;
    PW_PROBE_FILE *directory;
    UINT64 offset = 0;
    size_t done = 0;
    NTSTATUS status;
    (void)pattern; /* Pinned FSD always performs its own pattern filtering. */
    if (!fs || !fs->UserContext || !context || !transferred || (length && !buffer))
        return STATUS_INVALID_PARAMETER;
    volume = (PW_PROBE_VOLUME *)fs->UserContext;
    directory = (PW_PROBE_FILE *)context;
    if (directory->file && PwDirectoryContext(directory)) return STATUS_INVALID_PARAMETER;
    if (!directory->directory) return directory->directory_grant ? STATUS_NOT_SUPPORTED : STATUS_NOT_A_DIRECTORY;
    /* DirectoryMarkerAsNextOffset=1: marker points to 8 bytes, NOT a string.
     * Copy supports unaligned marker storage without dereferencing a UINT64*. */
    if (marker) CopyMemory(&offset, marker, sizeof(offset));
    status = PwNtStatus(pw_windows_directory_read(volume->volume, directory->directory,
        offset, (uint8_t *)buffer, length, &done));
    if (!NT_SUCCESS(status)) return status;
    *transferred = (ULONG)done; /* ABI caps to 16 MiB before encoding. */
    return STATUS_SUCCESS;
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
    callbacks->SetBasicInfo = PwSetBasicInfo;
    callbacks->ReadDirectory = PwReadDirectory;
    callbacks->Close = PwClose;
    callbacks->Flush = PwFlush;
    /* Create, Rename, ACL remain unset; no live directory Open or kernel refresh. */
}
#ifndef PW_WINFSP_NO_TEST
#define REQUIRE(condition) do { if (!(condition)) { \
    fprintf(stderr, "check failed: %s at line %d\n", #condition, __LINE__); \
    return 1; \
} } while (0)
/* Exercise real core refusals through the production error adapter. These
 * grants are cooperative C primitives, NOT LockFileEx/WinFsp lock dispatch. */
static int PwErrorPropagationContract(uint64_t volume, const PwCredentials *caller)
{
    int64_t object, aliasObject;
    uint64_t writer, reader, held, refused = 99;
    uint32_t maintenance = 99, protected = 99;
    PwStat before, after;
    PwMetadata oldMetadata, newMetadata;
    REQUIRE(PW_OK == pw_posix_create(volume, 1, (const uint8_t *)"error-source", 12, 0, 0600, 0, caller, &object));
    REQUIRE(PW_OK == pw_hard_link(volume, object, 1, (const uint8_t *)"error-alias", 11));
    REQUIRE(PW_OK == pw_posix_file_open(volume, 1, (const uint8_t *)"error-source", 12, 6, caller, &writer));
    REQUIRE(PW_OK == pw_posix_file_open(volume, 1, (const uint8_t *)"error-alias", 11, 4, caller, &reader));
    REQUIRE(PW_OK == pw_stat(volume, object, &before));
    REQUIRE(PW_OK == pw_metadata_get(volume, object, &oldMetadata));
    REQUIRE(PW_OK == pw_posix_file_lock_range(volume, writer, 0, 8, PW_LOCK_EXCLUSIVE, &held));
    REQUIRE(STATUS_FILE_LOCK_CONFLICT == PwNtStatus(pw_posix_file_lock_range(volume, reader, 0, 8, PW_LOCK_SHARED, &refused)));
    REQUIRE(refused == 99);
    /* Permission denial is distinct from a retryable conflict, even at the
     * same range. A READ-only grant cannot acquire an exclusive lock. */
    REQUIRE(STATUS_ACCESS_DENIED == PwNtStatus(pw_posix_file_lock_range(volume, reader, 0, 8, PW_LOCK_EXCLUSIVE, &refused)));
    REQUIRE(refused == 99);
    REQUIRE(PW_OK == pw_posix_file_unlock_range(volume, writer, held));
    REQUIRE(STATUS_SUCCESS == PwNtStatus(pw_posix_file_lock_range(volume, reader, 0, 8, PW_LOCK_SHARED, &refused)));
    REQUIRE(refused != 99 && PW_OK == pw_posix_file_unlock_range(volume, reader, refused));
    /* A wrong cached identity must preserve the actual alias and metadata. */
    REQUIRE(STATUS_RETRY == PwNtStatus(pw_remove_checked(volume, 1, (const uint8_t *)"error-alias", 11, 0, object + 1)));
    REQUIRE(PW_OK == pw_lookup(volume, 1, (const uint8_t *)"error-alias", 11, &aliasObject) && aliasObject == object);
    REQUIRE(PW_OK == pw_stat(volume, object, &after));
    REQUIRE(before.kind == after.kind && before.size == after.size && before.links == after.links && before.generation == after.generation);
    REQUIRE(PW_OK == pw_metadata_get(volume, object, &newMetadata));
    REQUIRE(0 == memcmp(&oldMetadata, &newMetadata, sizeof(oldMetadata)));
    REQUIRE(PW_OK == pw_health(volume, &maintenance, &protected) && !maintenance && !protected);
    REQUIRE(PW_OK == pw_posix_file_close(volume, reader));
    REQUIRE(PW_OK == pw_posix_file_close(volume, writer));
    REQUIRE(PW_OK == pw_remove_checked(volume, 1, (const uint8_t *)"error-alias", 11, 0, object));
    REQUIRE(PW_OK == pw_remove_checked(volume, 1, (const uint8_t *)"error-source", 12, 0, object));
    REQUIRE(PwNtStatus(PW_INTERNAL) == STATUS_INTERNAL_ERROR);
    REQUIRE(PwNtStatus(INT32_MIN) == STATUS_INTERNAL_ERROR && PwNtStatus(INT32_MAX) == STATUS_INTERNAL_ERROR);
    REQUIRE(PwNtStatus(PW_END) == STATUS_INTERNAL_ERROR);
    REQUIRE(PwNtStatus(PW_IO) == STATUS_IO_DEVICE_ERROR && PwNtStatus(PW_CORRUPT) == STATUS_DATA_ERROR);
    REQUIRE(PwNtStatus(PW_BAD_COOKIE) == STATUS_INVALID_PARAMETER);
    puts("WINFSP_ERROR_PROPAGATION_CONTRACT_OK (real core conflict/retry/denial; no OS lock/mount)");
    return 0;
}
/* Independent SDK-typed consumer of bytes emitted by Rust, including unaligned
 * user buffers. No WinFsp DLL/helper implementation is copied or simulated. */
static BOOL PwDecodeDirectory(const UINT8 *buffer, ULONG length, ULONG offset,
    FSP_FSCTL_DIR_INFO *info, WCHAR *name, PULONG next)
{
    UINT16 size;
    ULONG padded, units, i;
    if (offset > length || length - offset < sizeof(size)) return FALSE;
    CopyMemory(&size, buffer + offset, sizeof(size));
    ZeroMemory(info, sizeof(*info)); name[0] = 0;
    if (!size) { *next = offset + (ULONG)sizeof(size); return TRUE; }
    if (size < sizeof(*info) || (size - sizeof(*info)) % sizeof(WCHAR)) return FALSE;
    padded = (ULONG)FSP_FSCTL_DEFAULT_ALIGN_UP(size);
    if (padded > length - offset) return FALSE;
    units = (size - (ULONG)sizeof(*info)) / (ULONG)sizeof(WCHAR);
    if (units > 255) return FALSE;
    CopyMemory(info, buffer + offset, sizeof(*info));
    CopyMemory(name, buffer + offset + sizeof(*info), units * sizeof(WCHAR));
    name[units] = 0;
    for (i = size; i < padded; ++i) if (buffer[offset + i]) return FALSE;
    *next = offset + padded;
    return TRUE;
}
static int PwDirectoryContract(FSP_FILE_SYSTEM *fs,
    const FSP_FILE_SYSTEM_INTERFACE *callbacks, uint64_t volume, const PwCredentials *caller)
{
    PW_PROBE_FILE directory = {0}, regular = {0};
    FSP_FSCTL_DIR_INFO record;
    FSP_FSCTL_FILE_INFO info;
    UINT8 bytes[2049], marker[9];
    WCHAR name[256];
    const char emoji[] = "\xF0\x9F\x98\x80.txt";
    UINT64 cookie, saved;
    ULONG done, offset, next, records, seen;
    size_t written;
    uint64_t handle, root;
    int64_t folder, file, unicodeFile, branch;
    PwCredentials other = {999, 20, NULL, 0, 0};
    REQUIRE(PW_OK == pw_directory_create(volume, 1, (const uint8_t *)"enumeration", 11, &folder));
    REQUIRE(PW_OK == pw_file_create(volume, folder, (const uint8_t *)"file", 4, &file));
    REQUIRE(PW_OK == pw_file_open(volume, file, &handle));
    REQUIRE(PW_OK == pw_file_write(volume, handle, 0, (const uint8_t *)"abc", 3, &written));
    REQUIRE(PW_OK == pw_file_close(volume, handle));
    REQUIRE(PW_OK == pw_hard_link(volume, file, folder, (const uint8_t *)"z-alias", 7));
    REQUIRE(PW_OK == pw_file_create(volume, folder, (const uint8_t *)emoji, strlen(emoji), &unicodeFile));
    REQUIRE(PW_OK == pw_directory_create(volume, folder, (const uint8_t *)"branch", 6, &branch));
    root = 99;
    REQUIRE(PW_DENIED == pw_windows_directory_begin(volume, folder, &other, &root) && root == 99);
    REQUIRE(PW_OK == pw_windows_directory_begin(volume, folder, caller, &directory.directory));
    REQUIRE(PW_BUSY == pw_volume_close(volume));
    done = 99; memset(bytes, 0xa5, sizeof(bytes));
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(fs, &directory, NULL, NULL, bytes + 1, 111, &done));
    REQUIRE(done == 0 && bytes[1] == 0xa5);
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(fs, &directory, L"ignored-pattern", NULL, bytes + 1, 112, &done));
    REQUIRE(done == 112 && PwDecodeDirectory(bytes + 1, done, 0, &record, name, &next));
    REQUIRE(record.NextOffset == 1 && 0 == wcscmp(name, L".") && record.FileInfo.IndexNumber == (UINT64)folder);
    /* Resume via an intentionally unaligned 64-bit cookie, not a name. */
    cookie = record.NextOffset; CopyMemory(marker + 1, &cookie, sizeof(cookie));
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(fs, &directory, NULL, (PWSTR)(marker + 1), bytes + 1, 112, &done));
    REQUIRE(PwDecodeDirectory(bytes + 1, done, 0, &record, name, &next));
    REQUIRE(record.NextOffset == 2 && 0 == wcscmp(name, L"..") && record.FileInfo.IndexNumber == 1);
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(fs, &directory, L"does-not-match", NULL, bytes + 1, 2048, &done));
    offset = records = seen = 0;
    while (offset < done) {
        REQUIRE(PwDecodeDirectory(bytes + 1, done, offset, &record, name, &next));
        if (!record.Size) { REQUIRE(next == done); break; }
        ++records; REQUIRE(record.NextOffset == records && !record.FileInfo.HardLinks && !record.FileInfo.EaSize);
        if (0 == wcscmp(name, L"file") || 0 == wcscmp(name, L"z-alias")) {
            REQUIRE(record.FileInfo.IndexNumber == (UINT64)file && record.FileInfo.FileSize == 3 &&
                record.FileInfo.AllocationSize == 4096 && record.FileInfo.FileAttributes == FILE_ATTRIBUTE_NORMAL);
            seen |= 1;
        } else if (name[0] == 0xd83d && name[1] == 0xde00) {
            REQUIRE(record.FileInfo.IndexNumber == (UINT64)unicodeFile && 0 == wcscmp(name + 2, L".txt")); seen |= 2;
        } else {
            REQUIRE(record.FileInfo.FileAttributes == FILE_ATTRIBUTE_DIRECTORY);
            if (0 == wcscmp(name, L"branch")) REQUIRE(record.FileInfo.IndexNumber == (UINT64)branch);
        }
        offset = next;
    }
    REQUIRE(records == 6 && seen == 3 && !record.Size);
    cookie = 6; CopyMemory(marker + 1, &cookie, sizeof(cookie));
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(fs, &directory, NULL, (PWSTR)(marker + 1), bytes + 1, 2, &done));
    REQUIRE(done == 2 && bytes[1] == 0 && bytes[2] == 0);
    cookie = UINT64_MAX; CopyMemory(marker + 1, &cookie, sizeof(cookie)); done = 99;
    REQUIRE(STATUS_INVALID_PARAMETER == callbacks->ReadDirectory(fs, &directory, NULL, (PWSTR)(marker + 1), bytes + 1, 128, &done) && done == 99);
    REQUIRE(STATUS_INVALID_PARAMETER == callbacks->ReadDirectory(fs, &directory, NULL, NULL, NULL, 1, &done));
    REQUIRE(STATUS_NOT_A_DIRECTORY == callbacks->ReadDirectory(fs, &regular, NULL, NULL, bytes, 128, &done));
    REQUIRE(STATUS_ACCESS_DENIED == callbacks->GetFileInfo(fs, &directory, &info)); // no caller fallback
    directory.metadata_caller = caller; /* explicit lab READ+SEARCH, not OS attributes authorization */
    REQUIRE(STATUS_SUCCESS == callbacks->GetFileInfo(fs, &directory, &info));
    REQUIRE(info.IndexNumber == (UINT64)folder && info.FileAttributes == FILE_ATTRIBUTE_DIRECTORY);
    REQUIRE(STATUS_FILE_IS_A_DIRECTORY == callbacks->Read(fs, &directory, bytes, 0, 1, &done));
    directory.file = 1;
    REQUIRE(STATUS_INVALID_PARAMETER == callbacks->ReadDirectory(fs, &directory, NULL, NULL, bytes, 128, &done));
    directory.file = 0;
    /* The old metadata snapshot remains deterministic across namespace edits. */
    REQUIRE(PW_OK == pw_remove(volume, folder, (const uint8_t *)"file", 4, 0));
    REQUIRE(PW_OK == pw_remove(volume, folder, (const uint8_t *)"z-alias", 7, 0));
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(fs, &directory, NULL, NULL, bytes + 1, 2048, &done));
    REQUIRE(PwDecodeDirectory(bytes + 1, done, 224, &record, name, &next)); // branch after two dots
    REQUIRE(0 == wcscmp(name, L"branch"));
    REQUIRE(PW_OK == pw_windows_directory_begin(volume, 1, caller, &root));
    REQUIRE(PW_OK == pw_windows_directory_read(volume, root, 0, bytes + 1, 2048, &written));
    REQUIRE(PwDecodeDirectory(bytes + 1, (ULONG)written, 0, &record, name, &next));
    REQUIRE(0 == wcscmp(name, L"enumeration")); // root has no synthetic dot entries
    REQUIRE(PW_OK == pw_windows_directory_close(volume, root));
    saved = directory.directory; callbacks->Close(fs, &directory);
    REQUIRE(directory.directory == 0 && directory.close_status == PW_OK);
    written = 99;
    REQUIRE(PW_INVALID == pw_windows_directory_read(volume, saved, 0, bytes, 128, &written) && written == 99);
    puts("WINFSP_DIRECTORY_CONTRACT_OK (pinned SDK wire/callbacks; no directory Open/refresh/ACL/mount)");
    return 0;
}
static int PwDirectoryObservationContract(FSP_FILE_SYSTEM *fs,
    const FSP_FILE_SYSTEM_INTERFACE *callbacks, uint64_t volume, const PwCredentials *caller)
{
    PW_PROBE_FILE directory = {0};
    FSP_FSCTL_FILE_INFO info, saved;
    FSP_FSCTL_DIR_INFO record;
    PwWindowsFileInfo captured, current;
    PwMetadataUpdate update = {0};
    UINT8 bytes[256];
    WCHAR name[256];
    uint64_t old, fresh;
    int64_t object, replacement;
    ULONG done, next;
    REQUIRE(PW_OK == pw_directory_create(volume, 1, (const uint8_t *)"current", 7, &object));
    REQUIRE(PW_OK == pw_windows_directory_begin(volume, object, caller, &directory.directory));
    REQUIRE(PW_OK == pw_windows_directory_info(volume, directory.directory, caller, &captured));
    memset(&info, 0xa5, sizeof(info)); saved = info;
    REQUIRE(STATUS_ACCESS_DENIED == callbacks->GetFileInfo(fs, &directory, &info));
    REQUIRE(0 == memcmp(&info, &saved, sizeof(info)));
    directory.metadata_caller = caller;
    update.valid = PW_META_MODIFIED;
    update.modified.seconds = 10; update.modified.nanoseconds = 700;
    REQUIRE(PW_OK == pw_metadata_set(volume, object, &update));
    REQUIRE(STATUS_SUCCESS == callbacks->GetFileInfo(fs, &directory, &info));
    REQUIRE(info.IndexNumber == (UINT64)object && info.FileAttributes == FILE_ATTRIBUTE_DIRECTORY &&
        !info.FileSize && !info.AllocationSize && !info.HardLinks && !info.EaSize &&
        info.LastWriteTime == UINT64_C(116444736100000007));
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(fs, &directory, NULL, NULL, bytes, sizeof(bytes), &done));
    REQUIRE(PwDecodeDirectory(bytes, done, 0, &record, name, &next));
    REQUIRE(record.FileInfo.LastWriteTime == captured.last_write_time); // old listing, never GFI cache
    old = directory.directory;
    REQUIRE(PW_OK == pw_windows_directory_refresh(volume, old, caller, &fresh, &current));
    REQUIRE(old != fresh && current.last_write_time == info.LastWriteTime);
    directory.directory = fresh; // explicit contract restart; discard old cookies
    REQUIRE(PW_INVALID == pw_windows_directory_close(volume, old));
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(fs, &directory, NULL, NULL, bytes, sizeof(bytes), &done));
    REQUIRE(PwDecodeDirectory(bytes, done, 0, &record, name, &next));
    REQUIRE(record.FileInfo.LastWriteTime == current.last_write_time);
    update.valid = PW_META_MODE; update.mode = 0;
    REQUIRE(PW_OK == pw_metadata_set(volume, object, &update));
    saved = info;
    REQUIRE(STATUS_ACCESS_DENIED == callbacks->GetFileInfo(fs, &directory, &info));
    REQUIRE(0 == memcmp(&info, &saved, sizeof(info)));
    fresh = 99;
    REQUIRE(PW_DENIED == pw_windows_directory_refresh(volume, directory.directory, caller, &fresh, &current) && fresh == 99);
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(fs, &directory, NULL, NULL, bytes, sizeof(bytes), &done));
    REQUIRE(PW_OK == pw_remove(volume, 1, (const uint8_t *)"current", 7, 1));
    REQUIRE(PW_OK == pw_directory_create(volume, 1, (const uint8_t *)"current", 7, &replacement));
    REQUIRE(replacement != object);
    REQUIRE(STATUS_OBJECT_NAME_NOT_FOUND == callbacks->GetFileInfo(fs, &directory, &info));
    REQUIRE(0 == memcmp(&info, &saved, sizeof(info)));
    REQUIRE(PW_NOT_FOUND == pw_windows_directory_refresh(volume, directory.directory, caller, &fresh, &current));
    REQUIRE(fresh == 99);
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(fs, &directory, NULL, NULL, bytes, sizeof(bytes), &done));
    REQUIRE(PwDecodeDirectory(bytes, done, 0, &record, name, &next) && record.FileInfo.IndexNumber == (UINT64)object);
    callbacks->Close(fs, &directory);
    REQUIRE(!directory.directory && directory.close_status == PW_OK);
    puts("WINFSP_DIRECTORY_REFRESH_CONTRACT_OK (fresh trusted linked-directory info; explicit replacement; no live inode/ACL/mount)");
    return 0;
}
static int PwV3DirectoryGrantContract(const char *base, const FSP_FILE_SYSTEM_INTERFACE *callbacks)
{
    const char suffix[] = ".dir-v3";
    size_t length = strlen(base);
    char *path;
    FSP_FILE_SYSTEM fs = {0};
    PW_PROBE_VOLUME owner = {0};
    PW_PROBE_FILE directory = {0};
    PwCreateAttributes attributes = {100, 20, 0777, 0};
    PwCredentials caller = {100, 20, NULL, 0, 0};
    PwVolumeIdentity identity;
    PwFileInfo inode;
    PwStat stat;
    PwMetadataUpdate update = {0};
    FSP_FSCTL_FILE_INFO info, saved;
    UINT8 bytes[256];
    ULONG done;
    uint64_t old, empty, refused;
    int64_t object, replacement;
    REQUIRE(length < 32700);
    path = (char *)malloc(length + sizeof(suffix));
    REQUIRE(path != NULL);
    CopyMemory(path, base, length); CopyMemory(path + length, suffix, sizeof(suffix));
    REQUIRE(PW_OK == pw_volume_create_with_directory_lifetimes((const uint8_t *)path,
        length + sizeof(suffix) - 1, 65536, &attributes, &owner.volume));
    free(path);
    fs.UserContext = &owner;
    REQUIRE(PW_OK == pw_volume_identity(owner.volume, &identity) && identity.capabilities == 3);
    REQUIRE(PW_OK == pw_directory_create(owner.volume, 1, (const uint8_t *)"dir", 3, &object));
    REQUIRE(PW_OK == pw_posix_directory_open(owner.volume, object, &caller, &directory.directory_grant));
    REQUIRE(PW_OK == pw_windows_directory_begin_granted(owner.volume, directory.directory_grant, &directory.directory));
    REQUIRE(PW_BUSY == pw_volume_close(owner.volume));
    REQUIRE(STATUS_SUCCESS == callbacks->GetFileInfo(&fs, &directory, &info));
    REQUIRE(info.IndexNumber == (UINT64)object && info.FileAttributes == FILE_ATTRIBUTE_DIRECTORY && !info.FileSize && !info.AllocationSize);
    saved = info;
    REQUIRE(STATUS_ACCESS_DENIED == callbacks->SetBasicInfo(&fs, &directory, INVALID_FILE_ATTRIBUTES, 1, 0, 0, 0, &info));
    REQUIRE(0 == memcmp(&info, &saved, sizeof(info)));
    directory.metadata_caller = &caller;
    REQUIRE(STATUS_SUCCESS == callbacks->SetBasicInfo(&fs, &directory, INVALID_FILE_ATTRIBUTES,
        1, UINT64_C(116444736000000007), UINT64_C(116444735999999999), 0, &info));
    REQUIRE(info.CreationTime == 1 && info.LastAccessTime == UINT64_C(116444736000000007) &&
        info.LastWriteTime == UINT64_C(116444735999999999) && info.FileAttributes == FILE_ATTRIBUTE_DIRECTORY);
    update.valid = PW_META_MODE; update.mode = 0;
    REQUIRE(PW_OK == pw_metadata_set(owner.volume, object, &update));
    refused = 99;
    REQUIRE(PW_DENIED == pw_posix_directory_open(owner.volume, object, &caller, &refused) && refused == 99);
    REQUIRE(PW_OK == pw_remove(owner.volume, 1, (const uint8_t *)"dir", 3, 1));
    REQUIRE(PW_OK == pw_directory_create(owner.volume, 1, (const uint8_t *)"dir", 3, &replacement));
    REQUIRE(replacement != object);
    REQUIRE(STATUS_SUCCESS == callbacks->GetFileInfo(&fs, &directory, &info));
    REQUIRE(info.IndexNumber == (UINT64)object && info.CreationTime == 1);
    REQUIRE(PW_OK == pw_posix_directory_info(owner.volume, directory.directory_grant, &inode));
    REQUIRE(inode.object == object && inode.stat.links == 0 && inode.stat.kind == 1 && inode.metadata.mode == 0);
    REQUIRE(STATUS_SUCCESS == callbacks->SetBasicInfo(&fs, &directory, INVALID_FILE_ATTRIBUTES, 0, 0, 1, 0, &info));
    REQUIRE(info.LastWriteTime == 1 && info.IndexNumber == (UINT64)object);
    REQUIRE(PW_OK == pw_windows_directory_begin_granted(owner.volume, directory.directory_grant, &empty));
    REQUIRE(PW_OK == pw_windows_directory_close(owner.volume, directory.directory));
    directory.directory = empty; // explicit snapshot restart; discard old cookies
    REQUIRE(STATUS_SUCCESS == callbacks->ReadDirectory(&fs, &directory, NULL, NULL, bytes, sizeof(bytes), &done));
    REQUIRE(done == 2 && bytes[0] == 0 && bytes[1] == 0); // no . or .. after unlink
    REQUIRE(STATUS_FILE_IS_A_DIRECTORY == callbacks->Read(&fs, &directory, bytes, 0, 1, &done));
    update.valid = PW_META_UID; update.uid = 200;
    REQUIRE(PW_OK == pw_metadata_set(owner.volume, object, &update));
    REQUIRE(STATUS_SUCCESS == callbacks->GetFileInfo(&fs, &directory, &info)); saved = info;
    REQUIRE(STATUS_ACCESS_DENIED == callbacks->SetBasicInfo(&fs, &directory, INVALID_FILE_ATTRIBUTES, 0, 0, 2, 0, &info));
    REQUIRE(0 == memcmp(&info, &saved, sizeof(info)));
    {
        PwCredentials current = {200, 20, NULL, 0, 0};
        directory.metadata_caller = &current;
        REQUIRE(STATUS_SUCCESS == callbacks->SetBasicInfo(&fs, &directory, INVALID_FILE_ATTRIBUTES, 0, 0, 3, 0, &info));
        directory.metadata_caller = NULL;
    }
    old = directory.directory_grant;
    callbacks->Close(&fs, &directory);
    REQUIRE(!directory.directory && !directory.directory_grant && directory.close_status == PW_OK);
    REQUIRE(PW_NOT_FOUND == pw_stat(owner.volume, object, &stat));
    REQUIRE(PW_INVALID == pw_posix_directory_info(owner.volume, old, &inode));
    REQUIRE(PW_OK == pw_volume_close(owner.volume));
    puts("WINFSP_V3_DIRECTORY_GRANT_CONTRACT_OK (live trusted inode/metadata; no SID/ACL/Open/mount)");
    return 0;
}
int main(int argc, char **argv)
{
    FSP_FILE_SYSTEM_INTERFACE callbacks;
    FSP_FILE_SYSTEM fs = {0};
    FSP_FSCTL_VOLUME_INFO info = {0};
    FSP_FSCTL_FILE_INFO fileInfo = {0}, oldInfo;
    FSP_FSCTL_VOLUME_PARAMS params;
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
    PwVolumeParamsInit(&params);
    REQUIRE(params.DirectoryMarkerAsNextOffset && !params.PassQueryDirectoryPattern &&
        !params.PersistentAcls && !params.HardLinks);
    REQUIRE(callbacks.GetFileInfo == PwGetFileInfo && callbacks.Write == PwWrite &&
        callbacks.SetFileSize == PwSetFileSize && callbacks.SetBasicInfo == PwSetBasicInfo &&
        callbacks.ReadDirectory == PwReadDirectory && callbacks.Create == NULL);
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
    /* Real SetBasicInfo signature, trusted laboratory mode policy only. */
    REQUIRE(STATUS_SUCCESS == callbacks.GetFileInfo(&fs, &reader, &oldInfo));
    fileInfo = oldInfo;
    REQUIRE(STATUS_ACCESS_DENIED == callbacks.SetBasicInfo(&fs, &reader,
        INVALID_FILE_ATTRIBUTES, 1, 0, 0, 0, &fileInfo));
    REQUIRE(0 == memcmp(&fileInfo, &oldInfo, sizeof(fileInfo)));
    reader.metadata_caller = &caller; /* explicit claims, no daemon/root fallback */
    REQUIRE(STATUS_SUCCESS == callbacks.SetBasicInfo(&fs, &reader,
        INVALID_FILE_ATTRIBUTES, 1, UINT64_C(116444736000000007),
        UINT64_C(116444735999999999), 0, &fileInfo));
    REQUIRE(fileInfo.CreationTime == 1 &&
        fileInfo.LastAccessTime == UINT64_C(116444736000000007) &&
        fileInfo.LastWriteTime == UINT64_C(116444735999999999) &&
        fileInfo.IndexNumber == (UINT64)node && fileInfo.FileSize == 8);
    oldInfo = fileInfo;
    REQUIRE(STATUS_SUCCESS == callbacks.SetBasicInfo(&fs, &reader,
        INVALID_FILE_ATTRIBUTES, 0, 0, 0, 0, &fileInfo));
    REQUIRE(0 == memcmp(&fileInfo, &oldInfo, sizeof(fileInfo)));
    REQUIRE(STATUS_NOT_SUPPORTED == callbacks.SetBasicInfo(&fs, &reader,
        FILE_ATTRIBUTE_NORMAL, UINT64_C(116444736000000000), 0, 0, 0, &fileInfo));
    REQUIRE(STATUS_NOT_SUPPORTED == callbacks.SetBasicInfo(&fs, &reader,
        INVALID_FILE_ATTRIBUTES, 0, 0, UINT64_MAX, 0, &fileInfo));
    REQUIRE(STATUS_NOT_SUPPORTED == callbacks.SetBasicInfo(&fs, &reader,
        INVALID_FILE_ATTRIBUTES, 0, 0, UINT64_MAX - 1, 0, &fileInfo));
    REQUIRE(STATUS_NOT_SUPPORTED == callbacks.SetBasicInfo(&fs, &reader,
        INVALID_FILE_ATTRIBUTES, 0, 0, 0, 1, &fileInfo));
    REQUIRE(STATUS_INVALID_PARAMETER == callbacks.SetBasicInfo(&fs, &reader,
        INVALID_FILE_ATTRIBUTES, 0, 0, 0, 0, NULL));
    REQUIRE(0 == memcmp(&fileInfo, &oldInfo, sizeof(fileInfo)));
    REQUIRE(STATUS_SUCCESS == callbacks.GetFileInfo(&fs, &reader, &fileInfo));
    REQUIRE(0 == memcmp(&fileInfo, &oldInfo, sizeof(fileInfo)));
    /* A data writer who is no longer owner cannot change metadata. */
    file.metadata_caller = &caller;
    update.valid = PW_META_UID;
    update.uid = 101;
    REQUIRE(PW_OK == pw_metadata_set(volume, node, &update));
    REQUIRE(STATUS_SUCCESS == callbacks.GetFileInfo(&fs, &file, &oldInfo));
    fileInfo = oldInfo;
    REQUIRE(STATUS_ACCESS_DENIED == callbacks.SetBasicInfo(&fs, &file,
        INVALID_FILE_ATTRIBUTES, 0, 0, 1, 0, &fileInfo));
    REQUIRE(0 == memcmp(&fileInfo, &oldInfo, sizeof(fileInfo)));
    /* Still-open inode data and identity survive final unlink. */
    REQUIRE(PW_OK == pw_posix_remove(volume, 1, (const uint8_t *)"probe.txt", 9, 0, &caller));
    REQUIRE(STATUS_SUCCESS == callbacks.GetFileInfo(&fs, &file, &fileInfo));
    REQUIRE(fileInfo.IndexNumber == (UINT64)node);
    {
        PwCredentials current = {101, 20, NULL, 0, 0};
        file.metadata_caller = &current;
        REQUIRE(STATUS_SUCCESS == callbacks.SetBasicInfo(&fs, &file,
            INVALID_FILE_ATTRIBUTES, 0, 0, UINT64_C(116444736000000123), 0, &fileInfo));
        REQUIRE(fileInfo.LastWriteTime == UINT64_C(116444736000000123));
        file.metadata_caller = NULL;
    }
    callbacks.Close(&fs, &reader); callbacks.Close(&fs, &file);
    REQUIRE(reader.file == 0 && file.file == 0 && reader.close_status == PW_OK && file.close_status == PW_OK);
    REQUIRE(STATUS_INVALID_PARAMETER == callbacks.GetFileInfo(&fs, &file, &fileInfo));
    REQUIRE(PwDirectoryContract(&fs, &callbacks, volume, &caller) == 0);
    REQUIRE(PwDirectoryObservationContract(&fs, &callbacks, volume, &caller) == 0);
    REQUIRE(PwErrorPropagationContract(volume, &caller) == 0);
    REQUIRE(PW_OK == pw_volume_close(volume));
    REQUIRE(PwV3DirectoryGrantContract(argv[1], &callbacks) == 0);
    puts("WINFSP_BASIC_INFO_CONTRACT_OK (trusted current-owner policy; no Windows ACL/mount certification)");
    puts("WINFSP_GRANTED_IO_CONTRACT_OK (regular-file grants; no ACL/Open/mount certification)");
    puts("WINFSP_CALLBACK_CONTRACT_OK (no mount, no driver)");
    return 0;
}

#endif /* PW_WINFSP_NO_TEST */
