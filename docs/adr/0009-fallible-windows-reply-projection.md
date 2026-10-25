# ADR 0009 — Reject unsupported Windows replies before business commit

Date: 2026-10-25. Status: experimental granted-file callback contract; native security/mount gates closed.

The v2 core accepts signed Unix seconds and nanoseconds that are not all representable by Windows FILETIME. Returning a conversion error after a successful data mutation would violate the known-success/maintenance distinction. Silently saturating those times would also misrepresent stored metadata.

Extend transaction-captured replies with a pure fallible Rust projection before commit. The projection receives the core reply, never a database connection or another mutable borrow of the owner. It must have no external side effects. Projection refusal rolls back the business transaction; already staged unreferenced segments follow the existing cleanup/recovery rules. Business commit errors remain potentially indeterminate, and later maintenance errors remain separate warnings.

The native bridge's Windows projection floors nanoseconds to 100 ns, accepts FILETIME ticks in [0, INT64_MAX] and explicitly rejects other times. Stored metadata is unchanged. The lab dispatcher uses a 4096-byte logical allocation geometry; report rounded logical EOF, not physical backing usage or reserved space. Reject rounded values outside signed file limits. Allocation/preallocation setters remain unsupported and never quietly truncate or claim space was reserved.

WinFsp callback contexts now carry granted policy tokens, not unrestricted administrative handles. Open/GetSecurityByName remain denied; the synthetic contract program explicitly obtains its own test grant. File attributes are NORMAL without an invented Unix-mode-to-ACL translation. The pinned v2.1 FSP_FSCTL_FILE_INFO says HardLinks is unimplemented and must be zero; that interface gap remains a release blocker.

Tests distinguish pure projection/core/C ABI execution on Linux from real MSVC compilation/direct callback execution on Windows, and both from an actual installed-driver mount. No caller identity, ACL inheritance, share/cache/Flush or Codex sandbox gate is opened by these tests.

Primary sources reviewed 2026-10-25:
- Pinned callback and FILE_INFO declarations: https://github.com/winfsp/winfsp/blob/ddca7bd5481857a65ba552f643b8776fd070836f/inc/winfsp/winfsp.h and https://github.com/winfsp/winfsp/blob/ddca7bd5481857a65ba552f643b8776fd070836f/inc/winfsp/fsctl.h
- FILETIME epoch/units: https://learn.microsoft.com/en-us/windows/win32/api/minwinbase/ns-minwinbase-filetime
- Selected conversion range: https://learn.microsoft.com/en-us/windows/win32/api/timezoneapi/nf-timezoneapi-filetimetosystemtime
