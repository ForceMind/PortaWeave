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

@main
struct FSVolumeContract {
    static func main() throws {
        guard CommandLine.arguments.count == 2 else { throw ContractFailure(message: "new scratch path required") }
        let url = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
        let other = url.appendingPathExtension("other")
        try makeContainer(url)
        try makeContainer(other)
        let volume = try PortaWeaveVolume(laboratoryContainerURL: url)
        let second = try PortaWeaveVolume(laboratoryContainerURL: other)
        let root = volume.session.rootItem
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
        try require(names == ["b", "c"], "directory pagination skipped/repeated a row")
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
            try require(seen == ["b", "c", "d"], "repeated listing lost entries")
            if retainedVerifier == 0 { retainedVerifier = current }
            try require(current == retainedVerifier, "unchanged listing did not reuse its snapshot")
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
                    _, _, _, _, _ in throw ContractFailure(message: "empty directory was not empty")
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
                _, _, _, _, _ in throw ContractFailure(message: "new empty-directory listing changed")
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
        print("FSVOLUME_CALLBACK_CONTRACT_OK (SDK callbacks/core ABI; no installation or OS mount)")
    }
}
