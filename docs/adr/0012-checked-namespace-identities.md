# ADR 0012 — Check cached namespace identities in the mutation request

Date: 2026-10-26. Status: experimental trusted Rust/C/FSKit contract; native mount gates closed.

A lookup followed by a separate name-based mutation has no native request contract tying the cached FSItem to the inode being modified. Swift serializes its own queue, but another legitimate C owner request can replace a name between those calls. One engine/owner lock is still required; this is not concurrent independent Volume ownership or support for raw backing-file edits.

Add checked remove/rename entry points. Validate the expected source and optional destination under the same owner borrow/native mutex and recheck inside the actual SQLite mutation transaction. A missing or changed identity returns StaleEntry/PW_STALE, mapped to ESTALE by FSKit, before namespace/metadata changes or maintenance. Preserve existing type, directory-cycle, emptiness, open-inode and hard-link semantics.

An absent expected target requires absence. A same-entry case/normalization change is an explicit exception, while a distinct hard-link alias is still a different entry even when its inode matches. Matching two aliases keeps the existing no-op. These are inode-identity guards, not entry generations or full ABA-history detection. IDs are volume-scoped identifiers, not authentication or policy capabilities.

FSKit supplies the current session's validated item identities directly to the checked request. There is no fallible post-commit lookup/open, and known business success stays successful across later maintenance failure. Actual commit errors remain potentially indeterminate. Administrative raw APIs and trusted POSIX policy retain their distinct contracts; no caller/SID/ACL/share/Flush/mount gate opens.

Test the old lookup/new replacement event order with two real C-ABI threads, stale source/target/absence, unchanged metadata/content/health, v1/v2, Unicode/case, hard-link aliases, directory replacement and before/after-commit faults. Real SDK callback tests and artifact handover remain separate from an actual OS mount and normal Codex sandbox validation.
