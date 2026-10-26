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
#include <stddef.h>
#include <wchar.h>

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
    int32_t close_status;
    /* Test-only, lifetime-valid trusted claims. NULL denies metadata mutation.
     * Open never manufactures this context. Future native ACL authorization
     * must not infer these claims from daemon UID or untrusted request data. */
    const PwCredentials *metadata_caller;
} PW_PROBE_FILE;

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
    if (file->directory) return file->file ? STATUS_INVALID_PARAMETER : STATUS_NOT_SUPPORTED;
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
    if (file->directory) return file->file ? STATUS_INVALID_PARAMETER : STATUS_FILE_IS_A_DIRECTORY;
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
    if (file->directory) return file->file ? STATUS_INVALID_PARAMETER : STATUS_FILE_IS_A_DIRECTORY;
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
    if (file->directory) return file->file ? STATUS_INVALID_PARAMETER : STATUS_FILE_IS_A_DIRECTORY;
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
    if (file->directory) return file->file ? STATUS_INVALID_PARAMETER : STATUS_NOT_SUPPORTED;
    if (!file->metadata_caller) return STATUS_ACCESS_DENIED;
    ZeroMemory(&request, sizeof(request));
    request.file_attributes = attributes;
    request.creation_time = created;
    request.last_access_time = accessed;
    request.last_write_time = modified;
    request.change_time = changed;
    status = PwNtStatus(pw_windows_file_set_basic_info(volume->volume, file->file,
        file->metadata_caller, &request, &info));
    if (!NT_SUCCESS(status)) return status;
    /* Projection and authorization completed before commit. Copy cannot fail. */
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
    if (file->file && file->directory) { file->close_status = PW_INVALID; return; }
    if (file->directory) {
        file->close_status = pw_windows_directory_close(volume->volume, file->directory);
        if (file->close_status == PW_OK) file->directory = 0;
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
    if (directory->file && directory->directory) return STATUS_INVALID_PARAMETER;
    if (!directory->directory) return STATUS_NOT_A_DIRECTORY;
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
    /* Create, Rename, ACL etc. remain unset; no directory Open/GFI/refresh yet. */
}
#ifndef PW_WINFSP_NO_TEST
#define REQUIRE(condition) do { if (!(condition)) { \
    fprintf(stderr, "check failed: %s at line %d\n", #condition, __LINE__); \
    return 1; \
} } while (0)
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
    REQUIRE(STATUS_NOT_SUPPORTED == callbacks->GetFileInfo(fs, &directory, &info)); // no live directory GFI advertised
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
    REQUIRE(PW_OK == pw_volume_close(volume));
    puts("WINFSP_BASIC_INFO_CONTRACT_OK (trusted current-owner policy; no Windows ACL/mount certification)");
    puts("WINFSP_GRANTED_IO_CONTRACT_OK (regular-file grants; no ACL/Open/mount certification)");
    puts("WINFSP_CALLBACK_CONTRACT_OK (no mount, no driver)");
    return 0;
}

#endif /* PW_WINFSP_NO_TEST */
