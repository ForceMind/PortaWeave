// Actual FSKit objects + callback calls, not a registered or mounted volume.
import Foundation
import FSKit
import PortaWeaveNative

struct ContractFailure: Error { let message: String }

func require(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
    let result = try condition()
    if !result { throw ContractFailure(message: message) }
}

func waitError(_ operation: (@escaping (Error?) -> Void) -> Void) throws -> Error? {
    let done = DispatchSemaphore(value: 0)
    var result: Error?
    operation { result = $0; done.signal() }
    guard done.wait(timeout: .now() + 15) == .success else {
        throw ContractFailure(message: "callback did not complete")
    }
    return result
}

func success(_ operation: (@escaping (Error?) -> Void) -> Void) throws {
    if let error = try waitError(operation) { throw error }
}

func expect(_ code: POSIXErrorCode, _ operation: (@escaping (Error?) -> Void) -> Void) throws {
    let error = try waitError(operation)
    try require((error as? POSIXError)?.code == code, "expected \(code), got \(String(describing: error))")
}

func waitValue<T>(_ operation: (@escaping (T?, Error?) -> Void) -> Void) throws -> T {
    let done = DispatchSemaphore(value: 0)
    var value: T?
    var failure: Error?
    operation { value = $0; failure = $1; done.signal() }
    guard done.wait(timeout: .now() + 15) == .success else {
        throw ContractFailure(message: "value callback did not complete")
    }
    if let failure = failure { throw failure }
    guard let value = value else { throw ContractFailure(message: "empty successful callback") }
    return value
}

func create(_ volume: PortaWeaveVolume, _ name: String, _ parent: FSItem,
            type: FSItem.ItemType = .file) throws -> PortaWeaveItem {
    let attributes = FSItem.SetAttributesRequest()
    attributes.invalidateAllProperties()
    let item: FSItem = try waitValue { done in
        volume.createItem(named: FSFileName(string: name), type: type,
                          inDirectory: parent, attributes: attributes) { item, _, error in
            done(item, error)
        }
    }
    guard let result = item as? PortaWeaveItem else {
        throw ContractFailure(message: "unexpected item class")
    }
    return result
}

func lookup(_ volume: PortaWeaveVolume, _ name: String, _ parent: FSItem) throws -> PortaWeaveItem {
    let item: FSItem = try waitValue { done in
        volume.lookupItem(named: FSFileName(string: name), inDirectory: parent) { item, _, error in
            done(item, error)
        }
    }
    return item as! PortaWeaveItem
}

func write(_ volume: PortaWeaveVolume, _ item: FSItem, _ text: String) throws {
    let data = Data(text.utf8)
    let count: Int = try waitValue { done in
        volume.write(contents: data, to: item, at: 0) { count, error in done(count, error) }
    }
    try require(count == data.count, "short successful write")
}

// The SDK's mutable file buffer has no public constructor. Read back through
// the SAME session's actual C ABI; don't fake an FSKit buffer or claim dispatch.
func readBack(_ volume: PortaWeaveVolume, _ item: FSItem) throws -> Data {
    try volume.session.synchronized {
        let handle = try volume.session.fileHandle(item, requiring: .read)
        var data = [UInt8](repeating: 0, count: 128)
        var count = 0
        try pwCheck(pw_file_read(volume.session.volume, handle, 0, &data, data.count, &count))
        return Data(data.prefix(count))
    }
}

func makeContainer(_ url: URL) throws {
    var volume: UInt64 = 0
    let path = Data(url.path.utf8)
    try pwCheck(path.withUnsafeBytes {
        pw_volume_create($0.bindMemory(to: UInt8.self).baseAddress, path.count, 134_217_728, &volume)
    })
    try pwCheck(pw_volume_close(volume))
}

// Model another legitimate owner request after an SDK item was cached. Raw
// backing-file modification is NOT part of this test or the owner contract.
func replaceNativeName(_ volume: PortaWeaveVolume, _ from: String, _ to: String) throws {
    let a = Data(from.utf8)
    let b = Data(to.utf8)
    try volume.session.synchronized {
        try pwCheck(a.withUnsafeBytes { old in b.withUnsafeBytes { new in
            pw_rename(volume.session.volume, 1, old.bindMemory(to: UInt8.self).baseAddress, a.count,
                      1, new.bindMemory(to: UInt8.self).baseAddress, b.count, 1)
        } })
    }
}

@main
struct FSVolumeContract {
    // Real core failures passed through production pwCheck. No fcntl/flock,
    // system callback dispatch, mounted volume or OS identity is involved.
    static func errorPropagationContract(_ url: URL) throws {
        func expectStatus(_ status: Int32, _ code: POSIXErrorCode) throws {
            do {
                try pwCheck(status)
                throw ContractFailure(message: "native failure mapped to success: \(status)")
            } catch let error as POSIXError {
                try require(error.code == code, "native status \(status) mapped to \(error.code), expected \(code)")
            }
        }
        var native: UInt64 = 0
        var attributes = PwCreateAttributes(uid: 501, gid: 20, mode: 0o700, reserved: 0)
        var caller = PwCredentials(uid: 501, gid: 20, groups: nil, group_count: 0, reserved: 0)
        let path = Data(url.path.utf8)
        try pwCheck(path.withUnsafeBytes {
            pw_volume_create_with_metadata($0.bindMemory(to: UInt8.self).baseAddress,
                                          path.count, 65536, &attributes, &native)
        })
        let name = Data("source".utf8), alias = Data("alias".utf8)
        var object: Int64 = 0, actual: Int64 = 0
        try pwCheck(name.withUnsafeBytes {
            pw_posix_create(native, 1, $0.bindMemory(to: UInt8.self).baseAddress,
                            name.count, 0, 0o600, 0, &caller, &object)
        })
        try pwCheck(alias.withUnsafeBytes {
            pw_hard_link(native, object, 1, $0.bindMemory(to: UInt8.self).baseAddress, alias.count)
        })
        var writer: UInt64 = 0, reader: UInt64 = 0, held: UInt64 = 0, refused: UInt64 = 99
        try pwCheck(name.withUnsafeBytes {
            pw_posix_file_open(native, 1, $0.bindMemory(to: UInt8.self).baseAddress, name.count, 6, &caller, &writer)
        })
        try pwCheck(alias.withUnsafeBytes {
            pw_posix_file_open(native, 1, $0.bindMemory(to: UInt8.self).baseAddress, alias.count, 4, &caller, &reader)
        })
        var before = PwFileInfo(), after = PwFileInfo()
        try pwCheck(pw_posix_file_info(native, writer, &before))
        try pwCheck(pw_posix_file_lock_range(native, writer, 0, 8, UInt32(PW_LOCK_EXCLUSIVE), &held))
        try expectStatus(pw_posix_file_lock_range(native, reader, 0, 8, UInt32(PW_LOCK_SHARED), &refused), .EAGAIN)
        try require(refused == 99, "conflict published a lock token")
        try expectStatus(pw_posix_file_lock_range(native, reader, 0, 8, UInt32(PW_LOCK_EXCLUSIVE), &refused), .EACCES)
        try require(refused == 99, "denial published a lock token")
        try pwCheck(pw_posix_file_unlock_range(native, writer, held))
        try pwCheck(pw_posix_file_lock_range(native, reader, 0, 8, UInt32(PW_LOCK_SHARED), &refused))
        try require(refused != 99, "released conflict did not admit a fresh grant")
        try pwCheck(pw_posix_file_unlock_range(native, reader, refused))
        try expectStatus(alias.withUnsafeBytes {
            pw_remove_checked(native, 1, $0.bindMemory(to: UInt8.self).baseAddress, alias.count, 0, object + 1)
        }, .ESTALE)
        try pwCheck(alias.withUnsafeBytes {
            pw_lookup(native, 1, $0.bindMemory(to: UInt8.self).baseAddress, alias.count, &actual)
        })
        try require(actual == object, "stale refusal removed the actual alias")
        try pwCheck(pw_posix_file_info(native, writer, &after))
        try require(before.object == after.object && before.stat.generation == after.stat.generation &&
                    before.stat.links == after.stat.links && before.stat.size == after.stat.size &&
                    before.metadata.mode == after.metadata.mode && before.metadata.uid == after.metadata.uid &&
                    before.metadata.gid == after.metadata.gid &&
                    before.metadata.changed.seconds == after.metadata.changed.seconds &&
                    before.metadata.changed.nanoseconds == after.metadata.changed.nanoseconds,
                    "conflict/stale refusal changed inode state")
        var maintenance: UInt32 = 99, protected: UInt32 = 99
        try pwCheck(pw_health(native, &maintenance, &protected))
        try require(maintenance == 0 && protected == 0, "contention poisoned storage health")
        try pwCheck(pw_posix_file_close(native, reader))
        try pwCheck(pw_posix_file_close(native, writer))
        try pwCheck(pw_volume_close(native))
        for status in [Int32(PW_INTERNAL), Int32(PW_IO), Int32(PW_CORRUPT), Int32(PW_END), Int32.min, Int32.max] {
            try expectStatus(status, .EIO)
        }
        print("FSVOLUME_ERROR_PROPAGATION_CONTRACT_OK (real core conflict/retry/denial; no OS lock/mount)")
    }
    static func directoryMetadataSnapshotContract(_ base: URL) throws {
        for version in [1, 2, 3] {
            let url = base.appendingPathExtension("v\(version)")
            if version == 1 { try makeContainer(url) } else {
                let path = Data(url.path.utf8)
                var native: UInt64 = 0
                var attributes = PwCreateAttributes(uid: 501, gid: 20, mode: 0o700, reserved: 0)
                try pwCheck(path.withUnsafeBytes {
                    version == 3 ? pw_volume_create_with_directory_lifetimes($0.bindMemory(to: UInt8.self).baseAddress,
                        path.count, 134_217_728, &attributes, &native) :
                        pw_volume_create_with_metadata($0.bindMemory(to: UInt8.self).baseAddress,
                        path.count, 134_217_728, &attributes, &native)
                })
                try pwCheck(pw_volume_close(native))
            }
            let volume = try PortaWeaveVolume(laboratoryContainerURL: url)
            let root = volume.session.rootItem
            let first = try create(volume, "a", root)
            let last = try create(volume, "b", root)
            for item in [first, last] { try success { volume.openItem(item, modes: [.read, .write], replyHandler: $0) } }
            try write(volume, first, "aaa")
            try write(volume, last, "old")
            let request = FSItem.GetAttributesRequest()
            request.wantedAttributes = [.fileID, .size]
            if version != 1 { request.wantedAttributes.formUnion([.uid, .mode, .modifyTime]) }
            let before: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: last, replyHandler: $0) }
            var names: [String] = []
            var cookie: UInt64 = 0
            let token = try volume.session.synchronized {
                try volume.session.enumerate(root, cookie: 0, verifier: 0, attributes: request) { name, _, _, next, values in
                    guard names.isEmpty else { return false } // packer could not accept b
                    try require(name.string == "a" && values?.size == 3, "first captured row is wrong")
                    names.append(name.string!)
                    cookie = next
                    return true
                }
            }
            try require(cookie == 1, "partial pack lost the last accepted cookie")
            // Supported administrative calls, on the volume queue. No external
            // backing-file edits, raw fork, second owner or OS auth assumption.
            try volume.session.synchronized {
                let data = Data("new content".utf8)
                var count = 0
                try pwCheck(data.withUnsafeBytes { pw_file_write(volume.session.volume, last.handle!, 0,
                    $0.bindMemory(to: UInt8.self).baseAddress, data.count, &count) })
                if version != 1 {
                    var update = PwMetadataUpdate()
                    update.valid = UInt64(PW_META_MODE | PW_META_UID)
                    update.mode = 0
                    update.uid = 200
                    try pwCheck(pw_metadata_set(volume.session.volume, last.object, &update))
                }
                let name = Data("b".utf8)
                try pwCheck(name.withUnsafeBytes { pw_remove_checked(volume.session.volume, 1,
                    $0.bindMemory(to: UInt8.self).baseAddress, name.count, 0, last.object) })
            }
            try success { volume.closeItem(last, modes: [], replyHandler: $0) }
            try success { volume.reclaimItem(last, replyHandler: $0) }
            try volume.session.synchronized {
                var stat = PwStat()
                try require(pw_stat(volume.session.volume, last.object, &stat) == PW_NOT_FOUND, "snapshot pinned the removed child")
                _ = try volume.session.enumerate(root, cookie: cookie, verifier: token, attributes: request) { name, _, id, _, values in
                    try require(name.string == "b" && id.rawValue == UInt64(last.object) + 1 && values?.size == 3,
                                "old name was paired with current/replacement attributes")
                    if version == 1 {
                        try require(values != nil && !values!.isValid(.uid) && !values!.isValid(.mode), "v1 metadata was invented")
                    } else {
                        try require(values?.uid == before.uid && values?.mode == before.mode &&
                                    values?.modifyTime.tv_sec == before.modifyTime.tv_sec && values?.modifyTime.tv_nsec == before.modifyTime.tv_nsec,
                                    "snapshot metadata changed after overwrite/unlink/GC")
                    }
                    names.append(name.string!)
                    return true
                }
            }
            try require(names == ["a", "b"], "partial pack retry skipped or duplicated an entry")
            var freshNames: [String] = []
            let fresh = try volume.session.synchronized {
                try volume.session.enumerate(root, cookie: 0, verifier: 0, attributes: request) { name, _, _, _, _ in
                    freshNames.append(name.string!); return true
                }
            }
            try require(fresh != token && freshNames == ["a"], "new enumeration reused old captured names")
            do {
                _ = try volume.session.synchronized {
                    try volume.session.enumerate(root, cookie: 0, verifier: token, attributes: request) { _, _, _, _, _ in true }
                }
                throw ContractFailure(message: "retired verifier accepted")
            } catch let error as POSIXError { try require(error.code == .ESTALE, "wrong retired verifier error") }
            for pair in [(UInt64(99), fresh), (UInt64(1), UInt64(0))] {
                do {
                    _ = try volume.session.synchronized {
                        try volume.session.enumerate(root, cookie: pair.0, verifier: pair.1, attributes: request) { _, _, _, _, _ in true }
                    }
                    throw ContractFailure(message: "invalid directory cookie accepted")
                } catch {
                    let value = error as NSError
                    try require(value.domain == NSPOSIXErrorDomain && value.code == FSError.Code.invalidDirectoryCookie.rawValue,
                                "bad directory cookie used the wrong SDK error")
                }
            }
            var dots: [String] = []
            let dotToken = try volume.session.synchronized {
                try volume.session.enumerate(root, cookie: 0, verifier: 0, attributes: nil) { name, type, id, _, values in
                    if dots.count < 2 { try require(type == .directory && id.rawValue == 2, "root dots lost root identity") }
                    try require(values == nil, "nil attribute enumeration packed attributes")
                    dots.append(name.string!); return true
                }
            }
            try require(dots == [".", "..", "a"], "nil-attribute dot ordering is wrong")
            do {
                _ = try volume.session.synchronized {
                    try volume.session.enumerate(root, cookie: 0, verifier: dotToken, attributes: request) { _, _, _, _, _ in true }
                }
                throw ContractFailure(message: "changed dot mode reused ordinal cookies")
            } catch let error as POSIXError { try require(error.code == .ESTALE, "wrong dot mode error") }
            let parent = try create(volume, "parent", root, type: .directory)
            let directory = try create(volume, "directory", parent, type: .directory)
            var parentRows: [(String, UInt64)] = []
            _ = try volume.session.synchronized {
                try volume.session.enumerate(directory, cookie: 0, verifier: 0, attributes: nil) { name, _, id, _, _ in
                    parentRows.append((name.string!, id.rawValue)); return true
                }
            }
            try require(parentRows.count == 2 && parentRows[0].0 == "." && parentRows[0].1 == UInt64(directory.object) + 1 &&
                        parentRows[1].0 == ".." && parentRows[1].1 == UInt64(parent.object) + 1, "logical parent dot identity is wrong")
            try success { volume.removeItem(directory, named: FSFileName(string: "directory"), fromDirectory: parent, replyHandler: $0) }
            try success { volume.reclaimItem(directory, replyHandler: $0) }
            try success { volume.removeItem(parent, named: FSFileName(string: "parent"), fromDirectory: root, replyHandler: $0) }
            try success { volume.reclaimItem(parent, replyHandler: $0) }
            try success { volume.closeItem(first, modes: [], replyHandler: $0) }
            try success { volume.reclaimItem(first, replyHandler: $0) }
            try success { volume.deactivate(options: [], replyHandler: $0) }
        }
        print("FSVOLUME_CAPTURED_DIRECTORY_CONTRACT_OK (v1/v2/v3 names+metadata, GC, restart/dots/cookies/partial packing; no kernel dispatch/mount)")
    }
    static func directoryFailedRestartContract(_ url: URL) throws {
        let path = Data(url.path.utf8)
        var native: UInt64 = 0
        var attributes = PwCreateAttributes(uid: 501, gid: 20, mode: 0o700, reserved: 0)
        try pwCheck(path.withUnsafeBytes { pw_volume_create_with_metadata($0.bindMemory(to: UInt8.self).baseAddress,
            path.count, 134_217_728, &attributes, &native) })
        try pwCheck(pw_volume_close(native))
        let volume = try PortaWeaveVolume(laboratoryContainerURL: url)
        let directory = try create(volume, "directory", volume.session.rootItem, type: .directory)
        var child: Int64 = 0
        try volume.session.synchronized {
            let name = Data("child".utf8)
            try pwCheck(name.withUnsafeBytes { pw_file_create(volume.session.volume, directory.object,
                $0.bindMemory(to: UInt8.self).baseAddress, name.count, &child) })
        }
        let request = FSItem.GetAttributesRequest()
        request.wantedAttributes = [.fileID, .size]
        let token = try volume.session.synchronized {
            try volume.session.enumerate(directory, cookie: 0, verifier: 0, attributes: request) { _, _, _, _, _ in false }
        }
        try volume.session.synchronized {
            let childName = Data("child".utf8)
            let name = Data("directory".utf8)
            try pwCheck(childName.withUnsafeBytes { pw_remove_checked(volume.session.volume, directory.object,
                $0.bindMemory(to: UInt8.self).baseAddress, childName.count, 0, child) })
            try pwCheck(name.withUnsafeBytes { pw_remove_checked(volume.session.volume, 1,
                $0.bindMemory(to: UInt8.self).baseAddress, name.count, 1, directory.object) })
            var replacement: Int64 = 0
            try pwCheck(name.withUnsafeBytes { pw_directory_create(volume.session.volume, 1,
                $0.bindMemory(to: UInt8.self).baseAddress, name.count, &replacement) })
            try require(replacement != directory.object, "fixture reused a removed identity")
        }
        do {
            _ = try volume.session.synchronized {
                try volume.session.enumerate(directory, cookie: 0, verifier: 0, attributes: request) { _, _, _, _, _ in true }
            }
            throw ContractFailure(message: "restart followed the replacement name")
        } catch let error as POSIXError { try require(error.code == .ENOENT, "wrong removed-directory restart error") }
        var rows = 0
        _ = try volume.session.synchronized {
            try volume.session.enumerate(directory, cookie: 0, verifier: token, attributes: request) { name, _, id, _, values in
                try require(name.string == "child" && id.rawValue == UInt64(child) + 1 && values?.size == 0,
                            "failed restart destroyed old captured data")
                rows += 1; return true
            }
        }
        try require(rows == 1, "old verifier lost its rejected pack row")
        try success { volume.reclaimItem(directory, replyHandler: $0) }
        try success { volume.deactivate(options: [], replyHandler: $0) }
        print("FSVOLUME_FAILED_DIRECTORY_RESTART_CONTRACT_OK (same captured ID, preserved verifier/data; no native mount)")
    }
    static func directoryLifetimeContract(_ url: URL) throws {
        let path = Data(url.path.utf8)
        var native: UInt64 = 0
        var attributes = PwCreateAttributes(uid: 501, gid: 20, mode: 0o700, reserved: 0)
        try pwCheck(path.withUnsafeBytes {
            pw_volume_create_with_directory_lifetimes($0.bindMemory(to: UInt8.self).baseAddress,
                                                      path.count, 134_217_728, &attributes, &native)
        })
        var identity = PwVolumeIdentity()
        try pwCheck(pw_volume_identity(native, &identity))
        try require(identity.capabilities == 3, "v3 required directory capability absent")
        try pwCheck(pw_volume_close(native))
        let volume = try PortaWeaveVolume(laboratoryContainerURL: url)
        try require(volume.session.persistentIdentity == UUID(uuid: identity.uuid), "v3 persistent identity changed")
        let directory = try create(volume, "directory", volume.session.rootItem, type: .directory)
        var caller = PwCredentials(uid: 501, gid: 20, groups: nil, group_count: 0, reserved: 0)
        var grant: UInt64 = 0
        var cursor: UInt64 = 0
        try volume.session.synchronized {
            // Explicit external trusted pin; FSKit Open/caller mapping remains closed.
            try pwCheck(pw_posix_directory_open(volume.session.volume, directory.object, &caller, &grant))
            var update = PwMetadataUpdate()
            update.valid = UInt64(PW_META_MODE)
            update.mode = 0
            try pwCheck(pw_metadata_set(volume.session.volume, directory.object, &update))
            let name = Data("directory".utf8)
            try pwCheck(name.withUnsafeBytes {
                pw_remove_checked(volume.session.volume, 1, $0.bindMemory(to: UInt8.self).baseAddress,
                                  name.count, 1, directory.object)
            })
            var info = PwFileInfo()
            try pwCheck(pw_posix_directory_info(volume.session.volume, grant, &info))
            try require(info.object == directory.object && info.stat.links == 0, "removed directory inode lost")
            update.valid = UInt64(PW_META_MODIFIED)
            update.modified = PwTimestamp(seconds: 10, nanoseconds: 700, reserved: 0)
            try pwCheck(pw_posix_directory_set_metadata_info(volume.session.volume, grant, &caller, &update, &info))
            try pwCheck(pw_windows_directory_begin_granted(volume.session.volume, grant, &cursor))
            var bytes = [UInt8](repeating: 77, count: 8)
            var count = 0
            try pwCheck(pw_windows_directory_read(volume.session.volume, cursor, 0, &bytes, bytes.count, &count))
            try require(count == 2 && bytes[0] == 0 && bytes[1] == 0, "unlinked directory invented dot entries")
        }
        let request = FSItem.GetAttributesRequest()
        request.wantedAttributes = [.fileID, .uid, .mode, .modifyTime]
        let values: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: directory, replyHandler: $0) }
        try require(values.fileID.rawValue == UInt64(directory.object) + 1 && values.uid == 501 &&
                    values.mode == 0 && values.modifyTime.tv_sec == 10 && values.modifyTime.tv_nsec == 700,
                    "SDK attributes did not observe the retained v3 inode")
        try require(!values.isValid(.linkCount), "internal directory references advertised as native link count")
        try volume.session.synchronized {
            try pwCheck(pw_posix_directory_close(volume.session.volume, grant))
            var bytes = [UInt8](repeating: 77, count: 8)
            var count = 0
            try pwCheck(pw_windows_directory_read(volume.session.volume, cursor, 0, &bytes, bytes.count, &count))
            try require(count == 2, "snapshot incorrectly depended on the closed directory pin")
            try pwCheck(pw_windows_directory_close(volume.session.volume, cursor))
        }
        // The external grant is closed; the cached v3 SDK item now owns its
        // separate administrative reference until reclaim, not until modes=[].
        let retained: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: directory, replyHandler: $0) }
        try require(retained.fileID.rawValue == UInt64(directory.object) + 1 && directory.directoryHandle != nil,
                    "SDK cache lost its own directory reference")
        try expect(.EBUSY) { volume.deactivate(options: [], replyHandler: $0) }
        try success { volume.reclaimItem(directory, replyHandler: $0) }
        try expect(.ESTALE) { done in
            volume.getAttributes(request, of: directory) { _, error in done(error) }
        }
        try volume.session.synchronized {
            var stat = PwStat()
            try require(pw_stat(volume.session.volume, directory.object, &stat) == PW_NOT_FOUND,
                        "last SDK reference did not reclaim the empty orphan")
        }
        try success { volume.deactivate(options: [], replyHandler: $0) }
        print("FSVOLUME_V3_DIRECTORY_LIFETIME_CONTRACT_OK (external and SDK inode pins; no native authorization/mount)")
    }
    static func makeDirectoryVolume(_ url: URL) throws -> PortaWeaveVolume {
        let path = Data(url.path.utf8)
        var native: UInt64 = 0
        var attributes = PwCreateAttributes(uid: 501, gid: 20, mode: 0o750, reserved: 0)
        try pwCheck(path.withUnsafeBytes {
            pw_volume_create_with_directory_lifetimes($0.bindMemory(to: UInt8.self).baseAddress,
                                                      path.count, 134_217_728, &attributes, &native)
        })
        try pwCheck(pw_volume_close(native))
        let volume = try PortaWeaveVolume(laboratoryContainerURL: url)
        try require(volume.session.directoryLifetimes, "v3 lifetime profile lost")
        return volume
    }
    static func directoryItemContract(_ url: URL) throws {
        let volume = try makeDirectoryVolume(url)
        let root = volume.session.rootItem
        try require(root.directoryHandle == nil, "immortal root consumed an inode pin")
        let holder = try create(volume, "holder", root, type: .directory)
        let item = try create(volume, "active", root, type: .directory)
        try require(item.directoryHandle != nil && item.handle == nil && item.modes.isEmpty,
                    "directory creation did not publish a separate typed reference")
        try expect(.EISDIR) { volume.openItem(item, modes: .write, replyHandler: $0) }
        try success { volume.openItem(item, modes: .read, replyHandler: $0) }
        try success { volume.openItem(item, modes: .read, replyHandler: $0) }
        try expect(.EINVAL) { volume.closeItem(item, modes: .write, replyHandler: $0) }
        let _: FSFileName = try waitValue {
            volume.renameItem(item, inDirectory: root, named: FSFileName(string: "active"),
                              to: FSFileName(string: "moved"), inDirectory: holder, overItem: nil, replyHandler: $0)
        }
        try volume.session.synchronized {
            var update = PwMetadataUpdate()
            update.valid = UInt64(PW_META_MODE)
            update.mode = 0
            try pwCheck(pw_metadata_set(volume.session.volume, holder.object, &update))
        }
        // Administrative lab callbacks; mode changes do not certify OS access.
        try success { volume.removeItem(item, named: FSFileName(string: "moved"), fromDirectory: holder, replyHandler: $0) }
        for name in [".", "..", "absent"] {
            try expect(.ENOENT) { done in
                volume.lookupItem(named: FSFileName(string: name), inDirectory: item) { _, _, error in done(error) }
            }
        }
        let childAttributes = FSItem.SetAttributesRequest()
        childAttributes.invalidateAllProperties()
        try expect(.ENOENT) { done in
            volume.createItem(named: FSFileName(string: "no-new-children"), type: .file,
                              inDirectory: item, attributes: childAttributes) { _, _, error in done(error) }
        }
        try require(childAttributes.consumedAttributes.isEmpty, "orphan creation consumed fields")
        let request = FSItem.GetAttributesRequest()
        request.wantedAttributes = [.fileID, .mode, .modifyTime]
        let update = FSItem.SetAttributesRequest()
        update.invalidateAllProperties()
        update.mode = 0
        update.modifyTime = timespec(tv_sec: 123, tv_nsec: 456_789)
        let changed: FSItem.Attributes = try waitValue { volume.setAttributes(update, on: item, replyHandler: $0) }
        try require(changed.fileID.rawValue == UInt64(item.object) + 1 && changed.mode == 0 &&
                    changed.modifyTime.tv_sec == 123 && changed.modifyTime.tv_nsec == 456_789 &&
                    !changed.isValid(.linkCount), "removed directory metadata reply is wrong")
        try require(update.wasAttributeConsumed(.mode) && update.wasAttributeConsumed(.modifyTime), "directory update was not consumed")
        try volume.session.synchronized {
            _ = try volume.session.enumerate(item, cookie: 0, verifier: 0, attributes: nil) {
                _, _, _, _, _ in throw ContractFailure(message: "empty orphan emitted an invented entry")
            }
        }
        try expect(.EACCES) { done in volume.checkAccess(to: item, requestedAccess: .readData) { allowed, error in
            if allowed { done(ContractFailure(message: "directory pin enabled native access")) } else { done(error) }
        } }
        try expect(.EBUSY) { volume.reclaimItem(item, replyHandler: $0) }
        try expect(.EBUSY) { volume.deactivate(options: [], replyHandler: $0) }
        try success { volume.closeItem(item, modes: [], replyHandler: $0) }
        let retained: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: item, replyHandler: $0) }
        try require(retained.fileID.rawValue == changed.fileID.rawValue, "final close confused open access with cached identity")
        try success { volume.reclaimItem(item, replyHandler: $0) }
        try expect(.ESTALE) { done in volume.getAttributes(request, of: item) { _, error in done(error) } }
        try volume.session.synchronized {
            var stat = PwStat()
            try require(pw_stat(volume.session.volume, item.object, &stat) == PW_NOT_FOUND, "orphan survived final reference")
        }
        try success { volume.removeItem(holder, named: FSFileName(string: "holder"), fromDirectory: root, replyHandler: $0) }
        try success { volume.reclaimItem(holder, replyHandler: $0) }

        let source = try create(volume, "source", root, type: .directory)
        let target = try create(volume, "target", root, type: .directory)
        try success { volume.openItem(target, modes: .read, replyHandler: $0) }
        let _: FSFileName = try waitValue {
            volume.renameItem(source, inDirectory: root, named: FSFileName(string: "source"),
                              to: FSFileName(string: "target"), inDirectory: root, overItem: target, replyHandler: $0)
        }
        try require(try lookup(volume, "target", root) === source, "directory replacement lost source identity")
        let old: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: target, replyHandler: $0) }
        try require(old.fileID.rawValue == UInt64(target.object) + 1, "replacement followed the new name")
        try success { volume.closeItem(target, modes: [], replyHandler: $0) }
        try success { volume.reclaimItem(target, replyHandler: $0) }
        try success { volume.removeItem(source, named: FSFileName(string: "target"), fromDirectory: root, replyHandler: $0) }
        try success { volume.reclaimItem(source, replyHandler: $0) }

        try volume.session.synchronized {
            var object: Int64 = 0
            let name = Data("external".utf8)
            try pwCheck(name.withUnsafeBytes { pw_directory_create(volume.session.volume, 1, $0.bindMemory(to: UInt8.self).baseAddress, name.count, &object) })
        }
        let external = try lookup(volume, "external", root)
        let token = external.directoryHandle
        let sameExternal = try lookup(volume, "EXTERNAL", root)
        try require(token != nil && sameExternal === external && external.directoryHandle == token,
                    "directory lookup failed to retain/reuse a typed reference")
        try success { volume.removeItem(external, named: FSFileName(string: "external"), fromDirectory: root, replyHandler: $0) }
        try success { volume.reclaimItem(external, replyHandler: $0) }
        try success { volume.deactivate(options: [], replyHandler: $0) }
        print("FSVOLUME_DIRECTORY_ITEM_REFERENCE_CONTRACT_OK (v3 SDK create/lookup/open/unlink/replace/close/reclaim; no native authorization/mount)")
    }
    static func directoryAdmissionContract(_ url: URL) throws {
        let volume = try makeDirectoryVolume(url)
        let root = volume.session.rootItem
        let source = try create(volume, "source", root)
        var handles: [UInt64] = []
        try volume.session.synchronized {
            var object: Int64 = 0
            let name = Data("lookup-blocked".utf8)
            try pwCheck(name.withUnsafeBytes { pw_directory_create(volume.session.volume, 1, $0.bindMemory(to: UInt8.self).baseAddress, name.count, &object) })
            for _ in 0..<4095 {
                var handle: UInt64 = 0
                try pwCheck(pw_file_open(volume.session.volume, source.object, &handle))
                handles.append(handle)
            }
        }
        let request = FSItem.GetAttributesRequest()
        request.wantedAttributes = [.modifyTime, .changeTime]
        let before: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: root, replyHandler: $0) }
        let attributes = FSItem.SetAttributesRequest()
        attributes.invalidateAllProperties()
        attributes.mode = 0o700
        try expect(.ENOMEM) { done in volume.createItem(named: FSFileName(string: "absent"), type: .directory,
            inDirectory: root, attributes: attributes) { item, _, error in
                done(item == nil ? error : ContractFailure(message: "refused directory published an item"))
            }
        }
        try require(attributes.consumedAttributes.isEmpty, "directory admission consumed attributes")
        try expect(.ENOENT) { done in volume.lookupItem(named: FSFileName(string: "absent"), inDirectory: root) { _, _, error in done(error) } }
        try expect(.ENOMEM) { done in volume.lookupItem(named: FSFileName(string: "lookup-blocked"), inDirectory: root) { _, _, error in done(error) } }
        let after: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: root, replyHandler: $0) }
        try require(before.modifyTime.tv_sec == after.modifyTime.tv_sec && before.modifyTime.tv_nsec == after.modifyTime.tv_nsec &&
                    before.changeTime.tv_sec == after.changeTime.tv_sec && before.changeTime.tv_nsec == after.changeTime.tv_nsec,
                    "refused directory admission changed the parent")
        try volume.session.synchronized { try pwCheck(pw_file_close(volume.session.volume, handles.removeLast())) }
        let lookupItem = try lookup(volume, "lookup-blocked", root)
        try require(try lookup(volume, "LOOKUP-BLOCKED", root) === lookupItem, "cached lookup consumed another pin at full admission")
        try success { volume.reclaimItem(lookupItem, replyHandler: $0) }
        let resumed = try create(volume, "resumed", root, type: .directory)
        try require(resumed.directoryHandle != nil && resumed.handle == nil, "released capacity did not admit the directory atomically")
        try volume.session.synchronized {
            var refused: UInt64 = 99
            try require(pw_directory_open(volume.session.volume, 1, &refused) == PW_RESOURCE && refused == 99,
                        "created SDK directory did not occupy shared native admission")
        }
        try success { volume.openItem(resumed, modes: .read, replyHandler: $0) }
        try success { volume.removeItem(resumed, named: FSFileName(string: "resumed"), fromDirectory: root, replyHandler: $0) }
        try success { volume.closeItem(resumed, modes: [], replyHandler: $0) }
        try success { volume.reclaimItem(resumed, replyHandler: $0) }
        try volume.session.synchronized { for handle in handles { try pwCheck(pw_file_close(volume.session.volume, handle)) } }
        try success { volume.reclaimItem(source, replyHandler: $0) }
        try success { volume.deactivate(options: [], replyHandler: $0) }
        print("FSVOLUME_DIRECTORY_ADMISSION_CONTRACT_OK (real 4096 mixed file/directory pins and atomic SDK creation; no native mount)")
    }
    static func cursorPressureContract(_ url: URL) throws {
        let path = Data(url.path.utf8)
        var native: UInt64 = 0
        var attributes = PwCreateAttributes(uid: 501, gid: 20, mode: 0o700, reserved: 0)
        try pwCheck(path.withUnsafeBytes {
            pw_volume_create_with_metadata($0.bindMemory(to: UInt8.self).baseAddress,
                                           path.count, 134_217_728, &attributes, &native)
        })
        try pwCheck(pw_volume_close(native))
        let volume = try PortaWeaveVolume(laboratoryContainerURL: url)
        let cached = try create(volume, "cached", volume.session.rootItem, type: .directory)
        let fresh = try create(volume, "fresh", volume.session.rootItem, type: .directory)
        var old = try volume.session.synchronized {
            try volume.session.enumerate(cached, cookie: 0, verifier: 0, attributes: nil) {
                name, type, _, _, values in
                    try require([".", ".."].contains(name.string ?? "") && type == .directory && values == nil, "empty directory dot row is invalid")
                    return true
            }
        }
        var caller = PwCredentials(uid: 501, gid: 20, groups: nil, group_count: 0, reserved: 0)
        var external: [UInt64] = []
        var administrative: UInt64 = 0
        try volume.session.synchronized {
            for _ in 0..<62 {
                var token: UInt64 = 0
                try pwCheck(pw_windows_directory_begin(volume.session.volume, cached.object, &caller, &token))
                external.append(token)
            }
            try pwCheck(pw_dir_begin(volume.session.volume, cached.object, &administrative))
        }
        let beforeRestart = old
        old = try volume.session.synchronized {
            try volume.session.enumerate(cached, cookie: 0, verifier: 0, attributes: nil) { name, _, _, _, _ in
                try require([".", ".."].contains(name.string ?? ""), "restart changed empty directory")
                return true
            }
        }
        try require(old != beforeRestart, "full shared admission blocked atomic restart")
        do {
            _ = try volume.session.synchronized {
                try volume.session.enumerate(cached, cookie: 0, verifier: beforeRestart, attributes: nil) { _, _, _, _, _ in true }
            }
            throw ContractFailure(message: "retired restart verifier accepted")
        } catch let error as POSIXError { try require(error.code == .ESTALE, "wrong restart verifier error") }
        let current = try volume.session.synchronized {
            try volume.session.enumerate(fresh, cookie: 0, verifier: 0, attributes: nil) {
                name, type, _, _, values in
                    try require([".", ".."].contains(name.string ?? "") && type == .directory && values == nil, "empty directory dot row is invalid")
                    return true
            }
        }
        try require(current != old, "new snapshot reused an evicted token")
        do {
            _ = try volume.session.synchronized {
                try volume.session.enumerate(cached, cookie: 0, verifier: old, attributes: nil) { _, _, _, _, _ in true }
            }
            throw ContractFailure(message: "pressure-evicted verifier accepted")
        } catch let error as POSIXError { try require(error.code == .ESTALE, "wrong pressure eviction error") }
        try volume.session.synchronized {
            var bytes = [UInt8](repeating: 0, count: 512)
            var count = 0
            try pwCheck(pw_windows_directory_read(volume.session.volume, external[0], 0, &bytes, bytes.count, &count))
            try require(count == 226, "Swift evicted or altered an external Windows snapshot")
            var info = PwWindowsFileInfo()
            try pwCheck(pw_windows_directory_info(volume.session.volume, external[0], &caller, &info))
            try require(info.index_number == UInt64(cached.object), "directory observation changed identity")
            let old = external[0]
            var replacement: UInt64 = 0
            try pwCheck(pw_windows_directory_refresh(volume.session.volume, old, &caller, &replacement, &info))
            try require(replacement != old && pw_windows_directory_close(volume.session.volume, old) == PW_INVALID,
                        "full-capacity replacement did not retire old token")
            external[0] = replacement
            try pwCheck(pw_windows_directory_read(volume.session.volume, replacement, 0, &bytes, bytes.count, &count))
            try require(count == 226, "replacement evicted external state or changed empty directory")
            for token in external { try pwCheck(pw_windows_directory_close(volume.session.volume, token)) }
            try pwCheck(pw_dir_close(volume.session.volume, administrative))
        }
        try success { volume.deactivate(options: [], replyHandler: $0) }
        print("FSVOLUME_CURSOR_PRESSURE_CONTRACT_OK (own LRU eviction; external snapshots preserved; no OS mount)")
        print("FSVOLUME_DIRECTORY_REPLACEMENT_CONTRACT_OK (native replacement at full shared admission; no native open grant)")
    }
    static func checkedNamespaceContract(_ base: URL) throws {
        for v2 in [false, true] {
            let url = base.appendingPathExtension(v2 ? "v2" : "v1")
            if v2 {
                let path = Data(url.path.utf8)
                var native: UInt64 = 0
                var attributes = PwCreateAttributes(uid: 501, gid: 20, mode: 0o700, reserved: 0)
                try pwCheck(path.withUnsafeBytes {
                    pw_volume_create_with_metadata($0.bindMemory(to: UInt8.self).baseAddress,
                                                   path.count, 134_217_728, &attributes, &native)
                })
                try pwCheck(pw_volume_close(native))
            } else { try makeContainer(url) }
            let volume = try PortaWeaveVolume(laboratoryContainerURL: url)
            let root = volume.session.rootItem
            let source = try create(volume, "source", root)
            let target = try create(volume, "target", root)
            let replacement = try create(volume, "incoming", root)
            for item in [source, target, replacement] {
                try success { volume.openItem(item, modes: [.read, .write], replyHandler: $0) }
            }
            try write(volume, source, "source bytes")
            try write(volume, target, "old target")
            try write(volume, replacement, "new target")
            try replaceNativeName(volume, "incoming", "target")
            try expect(.ESTALE) {
                volume.removeItem(target, named: FSFileName(string: "target"), fromDirectory: root, replyHandler: $0)
            }
            try expect(.ESTALE) { done in
                volume.renameItem(source, inDirectory: root, named: FSFileName(string: "source"),
                                  to: FSFileName(string: "target"), inDirectory: root, overItem: target) { _, error in done(error) }
            }
            try expect(.ESTALE) { done in
                volume.renameItem(target, inDirectory: root, named: FSFileName(string: "target"),
                                  to: FSFileName(string: "absent"), inDirectory: root, overItem: nil) { _, error in done(error) }
            }
            try expect(.ESTALE) { done in
                volume.renameItem(source, inDirectory: root, named: FSFileName(string: "source"),
                                  to: FSFileName(string: "target"), inDirectory: root, overItem: nil) { _, error in done(error) }
            }
            try expect(.ESTALE) { done in
                volume.renameItem(source, inDirectory: root, named: FSFileName(string: "source"),
                                  to: FSFileName(string: "absent"), inDirectory: root, overItem: replacement) { _, error in done(error) }
            }
            try require(try lookup(volume, "target", root) === replacement, "stale request replaced the new target")
            try require(try readBack(volume, target) == Data("old target".utf8), "stale target lost its open pin")
            try require(try readBack(volume, source) == Data("source bytes".utf8), "stale rename changed source")
            let spelling: FSFileName = try waitValue {
                volume.renameItem(source, inDirectory: root, named: FSFileName(string: "source"),
                                  to: FSFileName(string: "SOURCE"), inDirectory: root, overItem: nil, replyHandler: $0)
            }
            try require(spelling.string == "SOURCE", "case-only checked rename failed")
            let _: FSFileName = try waitValue {
                volume.createLink(to: source, named: FSFileName(string: "alias"), inDirectory: root, replyHandler: $0)
            }
            try expect(.ESTALE) { done in
                volume.renameItem(source, inDirectory: root, named: FSFileName(string: "SOURCE"),
                                  to: FSFileName(string: "alias"), inDirectory: root, overItem: nil) { _, error in done(error) }
            }
            let _: FSFileName = try waitValue {
                volume.renameItem(source, inDirectory: root, named: FSFileName(string: "SOURCE"),
                                  to: FSFileName(string: "alias"), inDirectory: root, overItem: source, replyHandler: $0)
            }
            try require(try lookup(volume, "SOURCE", root) === source, "two-link rename ceased to be a no-op")
            let _: FSFileName = try waitValue {
                volume.renameItem(source, inDirectory: root, named: FSFileName(string: "SOURCE"),
                                  to: FSFileName(string: "target"), inDirectory: root, overItem: replacement, replyHandler: $0)
            }
            try require(try lookup(volume, "target", root) === source, "matching checked replacement failed")
            try require(try readBack(volume, replacement) == Data("new target".utf8), "replacement destroyed its old open inode")
            for item in [source, target, replacement] {
                try success { volume.closeItem(item, modes: [], replyHandler: $0) }
                try success { volume.reclaimItem(item, replyHandler: $0) }
            }
            let oldDirectory = try create(volume, "directory", root, type: .directory)
            let newDirectory = try create(volume, "new-directory", root, type: .directory)
            try replaceNativeName(volume, "new-directory", "directory")
            try expect(.ESTALE) {
                volume.removeItem(oldDirectory, named: FSFileName(string: "directory"), fromDirectory: root, replyHandler: $0)
            }
            try require(try lookup(volume, "directory", root) === newDirectory, "stale rmdir removed the new directory")
            try success { volume.reclaimItem(oldDirectory, replyHandler: $0) }
            try success { volume.removeItem(newDirectory, named: FSFileName(string: "directory"), fromDirectory: root, replyHandler: $0) }
            try success { volume.reclaimItem(newDirectory, replyHandler: $0) }
            try success { volume.deactivate(options: [], replyHandler: $0) }
        }
        print("FSVOLUME_CHECKED_NAMESPACE_CONTRACT_OK (transactional expected identities; no OS mount)")
    }
    static func metadataContract(_ url: URL) throws {
        var native: UInt64 = 0
        var attributes = PwCreateAttributes(uid: 501, gid: 20, mode: 0o750, reserved: 0)
        let path = Data(url.path.utf8)
        try pwCheck(path.withUnsafeBytes {
            pw_volume_create_with_metadata($0.bindMemory(to: UInt8.self).baseAddress,
                                           path.count, 134_217_728, &attributes, &native)
        })
        var identity = PwVolumeIdentity()
        try pwCheck(pw_volume_identity(native, &identity))
        let expectedUUID = UUID(uuid: identity.uuid)
        var object: Int64 = 0
        attributes = PwCreateAttributes(uid: UInt32.max, gid: 80, mode: 0o6751, reserved: 0)
        let name = Data("executable".utf8)
        try pwCheck(name.withUnsafeBytes {
            pw_node_create_with_metadata(native, pw_root_id(), $0.bindMemory(to: UInt8.self).baseAddress,
                                         name.count, 0, &attributes, &object)
        })
        var update = PwMetadataUpdate()
        update.valid = UInt64(PW_META_CREATED | PW_META_ACCESSED | PW_META_MODIFIED)
        update.created = PwTimestamp(seconds: -123, nanoseconds: 999_999_999, reserved: 0)
        update.accessed = update.created
        update.modified = update.created
        try pwCheck(pw_metadata_set(native, object, &update))
        try pwCheck(pw_volume_close(native))

        let volume = try PortaWeaveVolume(laboratoryContainerURL: url)
        try require(volume.session.persistentIdentity == expectedUUID, "v2 UUID lost on SDK bridge")
        try require(volume.supportedVolumeCapabilities.supportsPersistentObjectIDs, "v2 persistent IDs missing")
        try require(!volume.supportedVolumeCapabilities.doesNotSupportSettingFilePermissions, "v2 stored mode setter unavailable")
        let rootNoop = FSItem.SetAttributesRequest()
        rootNoop.invalidateAllProperties()
        let rootGeneration = try volume.session.synchronized { try volume.session.status(volume.session.rootItem).generation }
        let rootValues: FSItem.Attributes = try waitValue { volume.setAttributes(rootNoop, on: volume.session.rootItem, replyHandler: $0) }
        try require(rootValues.fileID.rawValue == 2 && rootValues.uid == 501 && rootValues.mode == 0o750, "root no-op lost captured metadata/identity")
        try require(rootNoop.consumedAttributes.isEmpty && !rootValues.isValid(.linkCount), "root no-op invented consumed fields/link count")
        try require(try volume.session.synchronized { try volume.session.status(volume.session.rootItem).generation } == rootGeneration, "no-op setter changed generation")
        let item = try lookup(volume, "EXECUTABLE", volume.session.rootItem)
        let request = FSItem.GetAttributesRequest()
        request.wantedAttributes = [.uid, .gid, .mode, .birthTime, .accessTime, .modifyTime, .changeTime]
        let result: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: item, replyHandler: $0) }
        try require(result.uid == UInt32.max && result.gid == 80 && result.mode == 0o6751, "ownership/mode changed")
        for time in [result.birthTime, result.accessTime, result.modifyTime] {
            try require(time.tv_sec == -123 && time.tv_nsec == 999_999_999, "timestamp precision/sign lost")
        }
        try require(result.isValid(.birthTime) && result.isValid(.mode), "metadata validity missing")
        try require(!result.isValid(.flags) && !result.isValid(.parentID), "unsupported attributes invented")
        let denied = FSItem.GetAttributesRequest()
        denied.wantedAttributes = [.mode, .flags]
        try expect(.ENOTSUP) { done in volume.getAttributes(denied, of: item) { _, error in done(error) } }
        let mutation = FSItem.SetAttributesRequest()
        mutation.invalidateAllProperties()
        mutation.flags = 1
        mutation.size = 100
        try expect(.ENOTSUP) { done in volume.setAttributes(mutation, on: item) { _, error in done(error) } }
        try require(mutation.consumedAttributes.isEmpty, "refused metadata request was consumed")
        try require(try volume.session.synchronized { try volume.session.status(item).size } == 0, "refusal truncated file")
        let closed = FSItem.SetAttributesRequest()
        closed.invalidateAllProperties()
        closed.mode = 0o700
        closed.size = 2
        try expect(.EACCES) { done in volume.setAttributes(closed, on: item) { _, error in done(error) } }
        try require(closed.consumedAttributes.isEmpty, "closed write consumed attributes")
        let invalid = FSItem.SetAttributesRequest()
        invalid.invalidateAllProperties()
        invalid.size = 2
        invalid.modifyTime = timespec(tv_sec: 1, tv_nsec: 1_000_000_000)
        try expect(.EINVAL) { done in volume.setAttributes(invalid, on: item) { _, error in done(error) } }
        try require(invalid.consumedAttributes.isEmpty, "invalid nanoseconds consumed")
        let ownership = FSItem.SetAttributesRequest()
        ownership.invalidateAllProperties()
        ownership.gid = 81
        let owned: FSItem.Attributes = try waitValue { volume.setAttributes(ownership, on: item, replyHandler: $0) }
        try require(owned.gid == 81 && ownership.wasAttributeConsumed(.gid), "metadata-only setter failed")
        try require(owned.fileID.rawValue == UInt64(item.object) + 1 && owned.isValid(.birthTime) && owned.birthTime.tv_nsec == 999_999_999, "metadata-only reply lost captured identity/precision")
        try success { volume.openItem(item, modes: [.read, .write], replyHandler: $0) }
        let generation = try volume.session.synchronized { try volume.session.status(item).generation }
        let mixed = FSItem.SetAttributesRequest()
        mixed.invalidateAllProperties()
        mixed.mode = 0o6751
        mixed.size = 2
        mixed.modifyTime = timespec(tv_sec: -456, tv_nsec: 123_456_789)
        let resized: FSItem.Attributes = try waitValue { volume.setAttributes(mixed, on: item, replyHandler: $0) }
        try require(resized.size == 2 && resized.mode == 0o751 && resized.modifyTime.tv_sec == -456 && resized.modifyTime.tv_nsec == 123_456_789, "mixed attributes lost atomic values")
        try require(mixed.wasAttributeConsumed(.mode) && mixed.wasAttributeConsumed(.size) && mixed.wasAttributeConsumed(.modifyTime), "committed fields not consumed")
        try require(try volume.session.synchronized { try volume.session.status(item).generation } == generation + 1, "mixed setter used multiple commits")
        try write(volume, item, "code")
        let changed: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: item, replyHandler: $0) }
        try require(changed.mode == 0o751 && changed.modifyTime.tv_sec != -123, "write metadata not reflected")
        try require(changed.accessTime.tv_sec == -123 && changed.accessTime.tv_nsec == 999_999_999, "atime was invented")
        let createAttributes = FSItem.SetAttributesRequest()
        createAttributes.invalidateAllProperties()
        createAttributes.uid = 503
        createAttributes.gid = 81
        createAttributes.mode = 0o751
        createAttributes.size = 4
        createAttributes.birthTime = timespec(tv_sec: -789, tv_nsec: 123_456_789)
        createAttributes.accessTime = timespec(tv_sec: -788, tv_nsec: 999_999_999)
        createAttributes.modifyTime = timespec(tv_sec: -787, tv_nsec: 987_654_321)
        let created: FSItem = try waitValue { done in
            volume.createItem(named: FSFileName(string: "created"), type: .file, inDirectory: volume.session.rootItem,
                              attributes: createAttributes) { item, _, error in done(item, error) }
        }
        let createdValues: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: created, replyHandler: $0) }
        try require(createdValues.uid == 503 && createdValues.gid == 81 && createdValues.mode == 0o751, "creation attributes not stored")
        try require(createAttributes.wasAttributeConsumed(.uid) && createAttributes.wasAttributeConsumed(.gid) && createAttributes.wasAttributeConsumed(.mode), "creation attrs not consumed")
        try require(createdValues.size == 4 && createdValues.birthTime.tv_sec == -789 && createdValues.birthTime.tv_nsec == 123_456_789 && createdValues.accessTime.tv_sec == -788 && createdValues.modifyTime.tv_nsec == 987_654_321, "initial creation state lost")
        for attribute in [FSItem.Attribute.size, .birthTime, .accessTime, .modifyTime] {
            try require(createAttributes.wasAttributeConsumed(attribute), "initial state not consumed")
        }
        try success { volume.openItem(created, modes: .read, replyHandler: $0) }
        try require(try readBack(volume, created) == Data(repeating: 0, count: 4), "initial sparse file not zero")
        try success { volume.closeItem(created, modes: [], replyHandler: $0) }
        try success { volume.removeItem(created, named: FSFileName(string: "created"), fromDirectory: volume.session.rootItem, replyHandler: $0) }
        try success { volume.reclaimItem(created, replyHandler: $0) }
        let badCreate = FSItem.SetAttributesRequest()
        badCreate.invalidateAllProperties()
        badCreate.mode = 0o751
        badCreate.changeTime = timespec(tv_sec: 1, tv_nsec: 0)
        try expect(.ENOTSUP) { done in
            volume.createItem(named: FSFileName(string: "absent"), type: .file, inDirectory: volume.session.rootItem,
                              attributes: badCreate) { _, _, error in done(error) }
        }
        try require(badCreate.consumedAttributes.isEmpty, "unsupported create partly consumed")
        let invalidInitial = FSItem.SetAttributesRequest()
        invalidInitial.invalidateAllProperties()
        invalidInitial.size = 4
        invalidInitial.birthTime = timespec(tv_sec: 1, tv_nsec: 1_000_000_000)
        try expect(.EINVAL) { done in volume.createItem(named: FSFileName(string: "absent"), type: .file,
            inDirectory: volume.session.rootItem, attributes: invalidInitial) { _, _, error in done(error) } }
        try require(invalidInitial.consumedAttributes.isEmpty, "invalid initial time partly consumed")
        try expect(.ENOENT) { done in volume.lookupItem(named: FSFileName(string: "absent"), inDirectory: volume.session.rootItem) { _, _, error in done(error) } }
        try success { done in volume.createLink(to: item, named: FSFileName(string: "alias"), inDirectory: volume.session.rootItem) { _, e in done(e) } }
        let alias = try lookup(volume, "alias", volume.session.rootItem)
        try require(alias === item, "v2 alias identity changed")
        var rows = 0
        _ = try volume.session.synchronized {
            try volume.session.enumerate(volume.session.rootItem, cookie: 0, verifier: 0, attributes: request) {
                _, _, _, _, attrs in
                try require(attrs?.uid == UInt32.max && attrs?.mode == 0o751, "enumeration lost metadata")
                rows += 1
                return true
            }
        }
        try require(rows == 2, "v2 enumeration count")
        try expect(.EACCES) { done in volume.checkAccess(to: item, requestedAccess: .readData) { allowed, error in
            done(allowed ? ContractFailure(message: "access gate opened") : error)
        } }
        try success { volume.closeItem(item, modes: [], replyHandler: $0) }
        try success { volume.reclaimItem(item, replyHandler: $0) }
        try success { volume.deactivate(options: [], replyHandler: $0) }
        let reopened = try PortaWeaveVolume(laboratoryContainerURL: url)
        try require(reopened.session.persistentIdentity == expectedUUID, "reopened UUID differs")
        try require(reopened.volumeID.isEqual(volume.volumeID), "FSVolume persistent identifier differs")
        try require(reopened.session.identity != volume.session.identity, "session tag incorrectly persisted")
        try success { reopened.deactivate(options: [], replyHandler: $0) }
        print("FSVOLUME_METADATA_CONTRACT_OK (v2 stored attributes/UUID; no OS permission or mount certification)")
        print("FSVOLUME_INITIAL_STATE_CONTRACT_OK (single creation size/times; no OS authorization or mount)")
        print("FSVOLUME_COMMITTED_ATTRIBUTES_CONTRACT_OK (precommit attribute replies; no post-commit query or native authorization)")
        print("FSVOLUME_SETATTR_CONTRACT_OK (atomic v2 callbacks/creation; caller authorization remains closed)")
    }
    static func creationContract(_ url: URL) throws {
        var native: UInt64 = 0
        var rootAttributes = PwCreateAttributes(uid: 501, gid: 20, mode: 0o750, reserved: 0)
        let path = Data(url.path.utf8)
        try pwCheck(path.withUnsafeBytes {
            pw_volume_create_with_metadata($0.bindMemory(to: UInt8.self).baseAddress,
                                           path.count, 134_217_728, &rootAttributes, &native)
        })
        try pwCheck(pw_volume_close(native))
        let volume = try PortaWeaveVolume(laboratoryContainerURL: url)
        let source = try create(volume, "source", volume.session.rootItem)
        try require(source.handle != nil && source.object > 1, "created file lost atomic pin/identity")
        try require(try lookup(volume, "SOURCE", volume.session.rootItem) === source, "created item was not cached")
        var handles: [UInt64] = []
        try volume.session.synchronized {
            for _ in 0..<4095 {
                var token: UInt64 = 0
                try pwCheck(pw_file_open(volume.session.volume, source.object, &token))
                handles.append(token)
            }
        }
        let request = FSItem.GetAttributesRequest()
        request.wantedAttributes = [.modifyTime, .changeTime]
        let before: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: volume.session.rootItem, replyHandler: $0) }
        let denied = FSItem.SetAttributesRequest()
        denied.invalidateAllProperties()
        denied.mode = 0o600
        try expect(.ENOMEM) { done in
            volume.createItem(named: FSFileName(string: "absent"), type: .file, inDirectory: volume.session.rootItem,
                              attributes: denied) { _, _, error in done(error) }
        }
        try require(denied.consumedAttributes.isEmpty, "failed admission consumed creation fields")
        try expect(.ENOENT) { done in volume.lookupItem(named: FSFileName(string: "absent"), inDirectory: volume.session.rootItem) { _, _, error in done(error) } }
        let after: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: volume.session.rootItem, replyHandler: $0) }
        try require(before.modifyTime.tv_sec == after.modifyTime.tv_sec && before.modifyTime.tv_nsec == after.modifyTime.tv_nsec && before.changeTime.tv_sec == after.changeTime.tv_sec && before.changeTime.tv_nsec == after.changeTime.tv_nsec, "failed admission changed parent times")
        let directory = try create(volume, "directory", volume.session.rootItem, type: .directory)
        try require(directory.handle == nil, "directory creation allocated a file pin")
        try volume.session.synchronized { for handle in handles { try pwCheck(pw_file_close(volume.session.volume, handle)) } }
        let resumed = try create(volume, "resumed", volume.session.rootItem)
        try expect(.EBUSY) { volume.deactivate(options: [], replyHandler: $0) }
        for item in [source, directory, resumed] { try success { volume.reclaimItem(item, replyHandler: $0) } }
        try success { volume.deactivate(options: [], replyHandler: $0) }
        let legacyURL = url.appendingPathExtension("v1")
        try makeContainer(legacyURL)
        let legacy = try PortaWeaveVolume(laboratoryContainerURL: legacyURL)
        let initial = FSItem.SetAttributesRequest()
        initial.invalidateAllProperties()
        initial.size = 5
        let initialItem: FSItem = try waitValue { done in
            legacy.createItem(named: FSFileName(string: "initial"), type: .file, inDirectory: legacy.session.rootItem,
                              attributes: initial) { item, _, error in done(item, error) }
        }
        try require(initial.wasAttributeConsumed(.size), "v1 initial size not consumed")
        try success { legacy.openItem(initialItem, modes: .read, replyHandler: $0) }
        try require(try readBack(legacy, initialItem) == Data(repeating: 0, count: 5), "v1 initial zeros lost")
        try success { legacy.closeItem(initialItem, modes: [], replyHandler: $0) }
        try success { legacy.reclaimItem(initialItem, replyHandler: $0) }
        let dirSize = FSItem.SetAttributesRequest()
        dirSize.invalidateAllProperties()
        dirSize.size = 0
        try expect(.ENOTSUP) { done in legacy.createItem(named: FSFileName(string: "absent"), type: .directory,
            inDirectory: legacy.session.rootItem, attributes: dirSize) { _, _, error in done(error) } }
        try require(dirSize.consumedAttributes.isEmpty, "directory size consumed")
        let invalidTime = FSItem.SetAttributesRequest()
        invalidTime.invalidateAllProperties()
        invalidTime.birthTime = timespec(tv_sec: 1, tv_nsec: 1_000_000_000)
        try expect(.ENOTSUP) { done in legacy.createItem(named: FSFileName(string: "absent"), type: .file,
            inDirectory: legacy.session.rootItem, attributes: invalidTime) { _, _, error in done(error) } }
        try success { legacy.deactivate(options: [], replyHandler: $0) }
        print("FSVOLUME_ATOMIC_CREATE_CONTRACT_OK (reserved grant/cache admission; no OS caller or mount certification)")
    }
    static func main() throws {
        do {
            try pwCheck(Int32(PW_DENIED))
            throw ContractFailure(message: "permission denial mapped to success")
        } catch let error as POSIXError {
            try require(error.code == .EACCES, "permission denial became storage write protection")
        }
        guard CommandLine.arguments.count == 2 else { throw ContractFailure(message: "new scratch path required") }
        let url = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
        try errorPropagationContract(url.appendingPathExtension("errors"))
        let other = url.appendingPathExtension("other")
        try makeContainer(url)
        try makeContainer(other)
        let volume = try PortaWeaveVolume(laboratoryContainerURL: url)
        let second = try PortaWeaveVolume(laboratoryContainerURL: other)
        let root = volume.session.rootItem
        try require(volume.session.persistentIdentity == nil, "v1 persistent UUID invented")
        try require(!volume.supportedVolumeCapabilities.supportsPersistentObjectIDs, "v1 persistent UUID advertised")
        try require(volume.supportedVolumeCapabilities.doesNotSupportSettingFilePermissions, "v1 mode setter advertised")
        let unavailable = FSItem.GetAttributesRequest()
        unavailable.wantedAttributes = [.uid, .birthTime]
        try expect(.ENOTSUP) { done in volume.getAttributes(unavailable, of: root) { _, error in done(error) } }
        try require(try root.wireID().rawValue == 2, "root identifier collision")

        let file = try create(volume, "MiXeD.txt", root)
        let alias = try lookup(volume, "mixed.TXT", root)
        try require(alias === file, "lookup lost stable object identity")
        let spelling: FSFileName = try waitValue { done in
            volume.lookupItem(named: FSFileName(string: "MIXED.TXT"), inDirectory: root) {
                _, canonical, error in done(canonical, error)
            }
        }
        try require(spelling.string == "MiXeD.txt", "canonical spelling was not preserved")
        try expect(.EACCES) { done in volume.write(contents: Data("denied".utf8), to: file, at: 0) { _, e in done(e) } }
        try success { volume.openItem(file, modes: [.read, .write], replyHandler: $0) }
        try write(volume, file, "payload")
        try require(try readBack(volume, file) == Data("payload".utf8), "wrong stored contents")

        let size = FSItem.SetAttributesRequest()
        size.invalidateAllProperties()
        size.size = 4
        let resized: FSItem.Attributes = try waitValue { volume.setAttributes(size, on: file, replyHandler: $0) }
        try require(resized.size == 4 && size.wasAttributeConsumed(.size), "size change was not consumed")
        try require(resized.fileID.rawValue == UInt64(file.object) + 1 && !resized.isValid(.uid), "v1 reply invented metadata or lost identity")
        let mode = FSItem.SetAttributesRequest()
        mode.invalidateAllProperties()
        mode.mode = 0o777
        try expect(.ENOTSUP) { done in volume.setAttributes(mode, on: file) { _, e in done(e) } }
        try require(try readBack(volume, file) == Data("payl".utf8), "unsupported attributes changed content")
        try expect(.ENOTSUP) { done in
            volume.createItem(named: FSFileName(string: "must-not-exist"), type: .file,
                              inDirectory: root, attributes: mode) { _, _, e in done(e) }
        }
        try expect(.ENOENT) { done in
            volume.lookupItem(named: FSFileName(string: "must-not-exist"), inDirectory: root) { _, _, e in done(e) }
        }
        try expect(.EACCES) { done in volume.checkAccess(to: file, requestedAccess: .readData) { allowed, e in
            precondition(!allowed); done(e)
        } }

        let linked: FSFileName = try waitValue {
            volume.createLink(to: file, named: FSFileName(string: "Alias.txt"), inDirectory: root, replyHandler: $0)
        }
        try require(linked.string == "Alias.txt", "wrong link name")
        try require(try lookup(volume, "ALIAS.TXT", root) === file, "hard link became a copy")
        let request = FSItem.GetAttributesRequest()
        request.wantedAttributes = [.fileID, .size, .linkCount]
        let stat: FSItem.Attributes = try waitValue { volume.getAttributes(request, of: file, replyHandler: $0) }
        try require(stat.linkCount == 2 && stat.fileID.rawValue > 2, "wrong file identity/link count")
        try expect(.ESTALE) { done in second.getAttributes(request, of: file) { _, e in done(e) } }

        try success { volume.removeItem(file, named: FSFileName(string: "MiXeD.txt"), fromDirectory: root, replyHandler: $0) }
        try success { volume.removeItem(file, named: FSFileName(string: "Alias.txt"), fromDirectory: root, replyHandler: $0) }
        try write(volume, file, "open unlinked")
        try require(try readBack(volume, file) == Data("open unlinked".utf8), "unlinked open object disappeared")
        try expect(.EBUSY) { volume.reclaimItem(file, replyHandler: $0) }
        try expect(.EBUSY) { volume.deactivate(options: [], replyHandler: $0) }
        try success { volume.closeItem(file, modes: [], replyHandler: $0) }
        try success { volume.reclaimItem(file, replyHandler: $0) }
        try expect(.ESTALE) { done in volume.getAttributes(request, of: file) { _, e in done(e) } }

        let a = try create(volume, "a", root)
        let b = try create(volume, "b", root)
        for item in [a, b] { try success { volume.openItem(item, modes: [.read, .write], replyHandler: $0) } }
        try write(volume, a, "new")
        try write(volume, b, "old")
        let renamed: FSFileName = try waitValue {
            volume.renameItem(a, inDirectory: root, named: FSFileName(string: "a"), to: FSFileName(string: "b"),
                              inDirectory: root, overItem: b, replyHandler: $0)
        }
        try require(renamed.string == "b", "wrong rename spelling")
        try require(try lookup(volume, "b", root) === a, "rename did not replace")
        try require(try readBack(volume, b) == Data("old".utf8), "replaced open object was destroyed")
        for item in [a, b] {
            try success { volume.closeItem(item, modes: [], replyHandler: $0) }
            try success { volume.reclaimItem(item, replyHandler: $0) }
        }

        let directory = try create(volume, "Tree", root, type: .directory)
        try require(try lookup(volume, "..", directory) === root, "logical parent escaped volume")
        try require(try lookup(volume, "..", root) === root, "root parent escaped backing boundary")
        try success { volume.openItem(directory, modes: .read, replyHandler: $0) }
        try expect(.EBUSY) { volume.removeItem(directory, named: FSFileName(string: "Tree"), fromDirectory: root, replyHandler: $0) }
        try success { volume.closeItem(directory, modes: [], replyHandler: $0) }
        let child = try create(volume, "child", directory)
        try expect(.ENOTEMPTY) { volume.removeItem(directory, named: FSFileName(string: "Tree"), fromDirectory: root, replyHandler: $0) }
        try success { volume.removeItem(child, named: FSFileName(string: "child"), fromDirectory: directory, replyHandler: $0) }
        try success { volume.reclaimItem(child, replyHandler: $0) }
        try success { volume.removeItem(directory, named: FSFileName(string: "Tree"), fromDirectory: root, replyHandler: $0) }
        try success { volume.reclaimItem(directory, replyHandler: $0) }

        let c = try create(volume, "c", root)
        var cookie: UInt64 = 0
        var names: [String] = []
        let verifier = try volume.session.synchronized {
            try volume.session.enumerate(root, cookie: 0, verifier: 0, attributes: nil) {
                name, _, _, next, _ in
                if !names.isEmpty { return false }
                names.append(name.string!)
                cookie = next
                return true
            }
        }
        _ = try volume.session.synchronized {
            try volume.session.enumerate(root, cookie: cookie, verifier: verifier, attributes: nil) {
                name, _, _, _, _ in names.append(name.string!); return true
            }
        }
        try require(names == [".", "..", "b", "c"], "directory pagination skipped/repeated a row")
        let d = try create(volume, "d", root)
        do {
            _ = try volume.session.synchronized {
                try volume.session.enumerate(root, cookie: cookie, verifier: verifier, attributes: nil) {
                    _, _, _, _, _ in true
                }
            }
            throw ContractFailure(message: "stale directory verifier accepted")
        } catch let error as POSIXError { try require(error.code == .ESTALE, "wrong stale verifier error") }
        try success { volume.reclaimItem(c, replyHandler: $0) }
        try success { volume.reclaimItem(d, replyHandler: $0) }
        var retainedVerifier: UInt64 = 0
        for _ in 0..<256 {
            var seen: [String] = []
            let current = try volume.session.synchronized {
                try volume.session.enumerate(root, cookie: 0, verifier: 0, attributes: nil) {
                    name, _, _, _, _ in seen.append(name.string!); return true
                }
            }
            try require(seen == [".", "..", "b", "c", "d"], "repeated listing lost entries")
            try require(current != retainedVerifier, "new enumeration did not get a fresh verifier")
            retainedVerifier = current
        }
        var firstDirectory: PortaWeaveItem?
        var firstVerifier: UInt64 = 0
        var directories: [PortaWeaveItem] = []
        for index in 0..<80 {
            directories.append(try create(volume, "scan\(index)", root, type: .directory))
        }
        for (index, directory) in directories.enumerated() {
            let token = try volume.session.synchronized {
                try volume.session.enumerate(directory, cookie: 0, verifier: 0, attributes: nil) {
                    name, type, _, _, values in
                    try require([".", ".."].contains(name.string ?? "") && type == .directory && values == nil, "empty directory dot row is invalid")
                    return true
                }
            }
            if index == 0 { firstDirectory = directory; firstVerifier = token }
        }
        do {
            _ = try volume.session.synchronized {
                try volume.session.enumerate(firstDirectory!, cookie: 0, verifier: firstVerifier, attributes: nil) {
                    _, _, _, _, _ in true
                }
            }
            throw ContractFailure(message: "evicted snapshot verifier accepted")
        } catch let error as POSIXError { try require(error.code == .ESTALE, "wrong evicted verifier error") }
        _ = try volume.session.synchronized {
            try volume.session.enumerate(firstDirectory!, cookie: 0, verifier: 0, attributes: nil) {
                name, type, _, _, values in
                    try require([".", ".."].contains(name.string ?? "") && type == .directory && values == nil, "empty directory dot row is invalid")
                    return true
            }
        }
        try success { volume.synchronize(flags: .wait, replyHandler: $0) }
        try expect(.ENOTSUP) { volume.synchronize(flags: .noWait, replyHandler: $0) }
        try success { volume.deactivate(options: [], replyHandler: $0) }
        try success { second.deactivate(options: [], replyHandler: $0) }
        var reopened: UInt64 = 0
        let path = Data(url.path.utf8)
        try pwCheck(path.withUnsafeBytes {
            pw_volume_open($0.bindMemory(to: UInt8.self).baseAddress, path.count, &reopened)
        })
        try pwCheck(pw_volume_close(reopened))
        try metadataContract(url.appendingPathExtension("metadata"))
        try creationContract(url.appendingPathExtension("creation"))
        try checkedNamespaceContract(url.appendingPathExtension("checked"))
        try cursorPressureContract(url.appendingPathExtension("cursor-pressure"))
        try directoryLifetimeContract(url.appendingPathExtension("directory-v3"))
        try directoryItemContract(url.appendingPathExtension("directory-items-v3"))
        try directoryAdmissionContract(url.appendingPathExtension("directory-admission-v3"))
        try directoryMetadataSnapshotContract(url.appendingPathExtension("captured-directories"))
        try directoryFailedRestartContract(url.appendingPathExtension("failed-directory-restart"))
        print("FSVOLUME_CALLBACK_CONTRACT_OK (SDK callbacks/core ABI; no installation or OS mount)")
    }
}
