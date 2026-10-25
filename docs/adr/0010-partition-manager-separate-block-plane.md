# ADR 0010 — Separate Disk Studio from the existing filesystem storage plane

Date: 2026-10-25. Status: accepted architectural boundary, not authorization for
physical disk writes. Tracking: GitHub Issue #28.

## Context
Existing .pwvol is a directory-backed virtual FILESYSTEM, with SQLite catalog,
immutable data segments, and OS FSKit/WinFsp filesystem adapters. Partition
tables are BLOCK device abstractions with LBAs, GPT/MBR and guest filesystems.
DiskGenius also handles partition recovery, imaging and physical media access.

## Decision
1. Retain current .pwvol on-disk formats, the existing storage engine, automatic
   GC and the mandatory macOS/Windows/Codex work-disk release gates.
2. Plan logical subvolumes in .pwvol separately from a new dynamic .pwdisk
   virtual BLOCK disk format. Never conflate a logical subvolume with GPT.
3. Disk Studio M0 is **read-only regular-file image inspection**. No raw
   physical drive opens, writes, auto repair, device helper or driver install.
4. GPT inspection validates UEFI main+backup headers, CRC and entry arrays,
   bounded allocations, geometry, GUIDs, no overlapping partitions. MBR
   inspect reports the four primary records, explicitly does not traverse EBR.
5. Any future virtual-disk write requires preview, shadow copy, backup,
   fault-injection and verified commit. Any physical-drive write requires
   a separate least-privilege helper, verified target, exclusive offline
   state, independent backup and explicit user approval.
6. Formatting, resizing, filesystem mounts, cloning and data recovery are
   different engines and must earn their own format/OS-specific safety gates.
7. DiskGenius is a feature reference, not copied code or branded interface.

## Rejected shortcuts
- Fake partitions inside current FSKit/WinFsp file-volume interface.
- Silent conversion of user .pwvol into a partitionable raw disk.
- Automatically modifying real internal/system/EFI drives or opening
  arbitrary device paths with root/administrator powers.
- Claiming GPT edit support means full native APFS/NTFS/ext4 cross-platform I/O.
- Calling an EBR-blind, image-only read parser a complete partition manager.

## Consequences
Create independent Disk Studio documents and safety tests; original work-disk
development proceeds. No physical media touched, no privileged drivers
installed, no risky disk operations implemented without separate approval.
