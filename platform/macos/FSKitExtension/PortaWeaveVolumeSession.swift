// Native adapter contract, not an installed/mounted filesystem. The unary
// loader remains closed until metadata, permissions and platform gates pass.
import Foundation
import FSKit
import OSLog
import PortaWeaveNative

func pwCheck(_ status: Int32) throws {
    guard status != PW_OK else { return }
    let code: POSIXErrorCode
    switch Int(status) {
    case PW_NOT_FOUND: code = .ENOENT
    case PW_EXISTS: code = .EEXIST
    case PW_NOT_DIR: code = .ENOTDIR
    case PW_IS_DIR: code = .EISDIR
    case PW_NOT_EMPTY: code = .ENOTEMPTY
    case PW_BUSY: code = .EBUSY
    case PW_NO_SPACE: code = .ENOSPC
    case PW_QUOTA: code = .EDQUOT
    case PW_READ_ONLY: code = .EROFS
    case PW_UNSUPPORTED: code = .ENOTSUP
    case PW_RESOURCE: code = .ENOMEM
    case PW_BUFFER_TOO_SMALL: code = .EOVERFLOW
    case PW_INVALID: code = .EINVAL
    default: code = .EIO
    }
    throw POSIXError(code)
}

final class PortaWeaveItem: FSItem {
    let owner: UUID
    let object: Int64
    let kind: UInt32
    var handle: UInt64?
    var modes: FSVolume.OpenModes = []

    init(owner: UUID, object: Int64, kind: UInt32, handle: UInt64? = nil) {
        self.owner = owner
        self.object = object
        self.kind = kind
        self.handle = handle
        super.init()
    }

    var itemType: FSItem.ItemType { kind == 0 ? .file : .directory }

    // Core ROOT=1; FSKit reserves 1=parentOfRoot and 2=rootDirectory.
    func wireID() throws -> FSItem.Identifier {
        guard object > 0, let result = FSItem.Identifier(rawValue: UInt64(object) + 1) else {
            throw POSIXError(.EOVERFLOW)
        }
        return result
    }
}

/// Every mutable field and native call is confined to queue after init.
/// Replies are invoked on this serial queue; nested operations must enqueue,
/// not synchronously reenter it. This orders callbacks including barriers.
final class PortaWeaveVolumeSession {
    let identity: UUID
    let rootItem: PortaWeaveItem
    let queue = DispatchQueue(label: "dev.portaweave.volume")
    private let queueKey = DispatchSpecificKey<UUID>()
    private let logger = Logger(subsystem: "dev.portaweave.fs", category: "Volume")
    private(set) var volume: UInt64 = 0
    private var items: [Int64: PortaWeaveItem] = [:]
    private var cursors: [UInt64: (directory: Int64, cursor: UInt64)] = [:]
    private var cursorOrder: [UInt64] = []
    private let maximumCachedDirectorySnapshots = 32
    private(set) var capacity = PwSpace()
    private(set) var lastError: Error?

    init(laboratoryContainerURL url: URL) throws {
        let identity = UUID()
        self.identity = identity
        rootItem = PortaWeaveItem(owner: identity, object: pw_root_id(), kind: 1)
        queue.setSpecific(key: queueKey, value: identity)
        let path = Data(url.path.utf8)
        let status = path.withUnsafeBytes { bytes in
            pw_volume_open(bytes.bindMemory(to: UInt8.self).baseAddress, path.count, &volume)
        }
        try pwCheck(status)
        do {
            try pwCheck(pw_volume_space(volume, &capacity))
            items[pw_root_id()] = rootItem
        } catch {
            _ = pw_volume_close(volume)
            volume = 0
            throw error
        }
    }

    deinit {
        if volume != 0 {
            // A failed close deliberately leaves the owner in the C table;
            // never forcibly release a container still owning live handles.
            let status = pw_volume_close(volume)
            if status != PW_OK { logger.error("Unclosed native owner, status \(status)") }
        }
    }

    func perform(_ operation: @escaping () -> Void) {
        queue.async(execute: DispatchWorkItem(block: operation))
    }

    func synchronized<T>(_ operation: () throws -> T) rethrows -> T {
        if DispatchQueue.getSpecific(key: queueKey) == identity { return try operation() }
        return try queue.sync(execute: operation)
    }

    func record(_ error: Error) {
        lastError = error
        logger.error("Native operation failed: \(String(describing: error))")
    }

    func noteHealth() {
        var maintenance: UInt32 = 0
        var protected: UInt32 = 0
        let status = pw_health(volume, &maintenance, &protected)
        if status != PW_OK || maintenance != 0 || protected != 0 {
            logger.warning("Backend health status=\(status), maintenance=\(maintenance), protection=\(protected)")
            // Preserve the business success. Health warnings cannot turn a
            // committed create/write/rename into an apparent rollback.
        }
    }

    func requireOpen() throws {
        guard volume != 0 else { throw POSIXError(.ENXIO) }
    }

    func item(_ raw: FSItem) throws -> PortaWeaveItem {
        try requireOpen()
        guard let item = raw as? PortaWeaveItem, item.owner == identity,
              items[item.object] === item else { throw POSIXError(.ESTALE) }
        return item
    }

    func status(_ item: PortaWeaveItem) throws -> PwStat {
        _ = try self.item(item)
        var stat = PwStat()
        try pwCheck(pw_stat(volume, item.object, &stat))
        return stat
    }

    func data(_ name: FSFileName) throws -> Data {
        guard let string = name.string, !string.isEmpty else { throw POSIXError(.EILSEQ) }
        return Data(string.utf8)
    }

    func admitItem() throws {
        guard items.count < 4096 else { throw POSIXError(.EMFILE) }
    }

    func intern(_ object: Int64, kind: UInt32) throws -> PortaWeaveItem {
        if let found = items[object] { return found }
        try admitItem()
        var handle: UInt64 = 0
        if kind == 0 { try pwCheck(pw_file_open(volume, object, &handle)) }
        let item = PortaWeaveItem(
            owner: identity, object: object, kind: kind, handle: kind == 0 ? handle : nil
        )
        items[object] = item
        return item
    }

    func lookup(_ name: FSFileName, in directory: FSItem) throws -> (PortaWeaveItem, FSFileName) {
        let parent = try item(directory)
        guard parent.kind == 1 else { throw POSIXError(.ENOTDIR) }
        if name.string == "." { return (parent, FSFileName(string: ".")) }
        if name.string == ".." {
            var id: Int64 = 0
            try pwCheck(pw_parent_directory(volume, parent.object, &id))
            return (try intern(id, kind: 1), FSFileName(string: ".."))
        }
        let query = try data(name)
        var bytes = [UInt8](repeating: 0, count: 255)
        var count = 0
        var object: Int64 = 0
        let result = query.withUnsafeBytes { source in
            pw_lookup_entry(volume, parent.object, source.bindMemory(to: UInt8.self).baseAddress,
                            query.count, &bytes, bytes.count, &count, &object)
        }
        try pwCheck(result)
        var stat = PwStat()
        try pwCheck(pw_stat(volume, object, &stat))
        let canonical = FSFileName(data: Data(bytes.prefix(count)))
        return (try intern(object, kind: stat.kind), canonical)
    }

    func attributes(_ request: FSItem.GetAttributesRequest, of raw: FSItem) throws -> FSItem.Attributes {
        let item = try self.item(raw)
        var supported: FSItem.Attribute = [.type, .size, .fileID]
        if item.kind == 0 { supported.insert(.linkCount) }
        guard request.wantedAttributes.subtracting(supported).isEmpty else {
            throw POSIXError(.ENOTSUP)
        }
        let stat = try status(item)
        let result = FSItem.Attributes()
        result.invalidateAllProperties()
        result.type = item.itemType
        result.size = stat.size
        result.fileID = try item.wireID()
        if item.kind == 0 {
            guard let count = UInt32(exactly: stat.links) else { throw POSIXError(.EOVERFLOW) }
            result.linkCount = count
        }
        return result
    }

    func invalidateEnumerations() {
        for value in cursors.values {
            let status = pw_dir_close(volume, value.cursor)
            if status != PW_OK { record(POSIXError(.EIO)) }
        }
        cursors.removeAll()
        cursorOrder.removeAll()
    }

    func reclaim(_ raw: FSItem) throws {
        let item = try self.item(raw)
        guard item.modes.isEmpty else { throw POSIXError(.EBUSY) }
        for (key, value) in Array(cursors) where value.directory == item.object {
            try pwCheck(pw_dir_close(volume, value.cursor))
            cursors.removeValue(forKey: key)
            cursorOrder.removeAll { $0 == key }
        }
        if let handle = item.handle {
            try pwCheck(pw_file_close(volume, handle))
            item.handle = nil
            noteHealth()
        }
        items.removeValue(forKey: item.object)
    }

    func close() throws {
        try requireOpen()
        // File identities remain pinned until FSKit explicitly reclaims them.
        guard items.values.allSatisfy({ $0.handle == nil && $0.modes.isEmpty }) else {
            throw POSIXError(.EBUSY)
        }
        invalidateEnumerations()
        var sync = PwStorageSync()
        try pwCheck(pw_storage_synchronize(volume, &sync))
        try pwCheck(pw_volume_close(volume))
        volume = 0
        items.removeAll()
    }

    func fileHandle(_ raw: FSItem, requiring mode: FSVolume.OpenModes) throws -> UInt64 {
        let item = try self.item(raw)
        guard item.kind == 0, let handle = item.handle else { throw POSIXError(.EISDIR) }
        guard item.modes.contains(mode) else { throw POSIXError(.EACCES) }
        return handle
    }

    func enumerate(
        _ directory: FSItem, cookie: UInt64, verifier: UInt64,
        attributes: FSItem.GetAttributesRequest?,
        pack: (FSFileName, FSItem.ItemType, FSItem.Identifier, UInt64, FSItem.Attributes?) throws -> Bool
    ) throws -> UInt64 {
        let directory = try item(directory)
        guard directory.kind == 1 else { throw POSIXError(.ENOTDIR) }
        // No snapshot is allocated for a request needing missing metadata.
        if let request = attributes {
            let allowed: FSItem.Attribute = [.type, .size, .fileID]
            guard request.wantedAttributes.subtracting(allowed).isEmpty else {
                throw POSIXError(.ENOTSUP)
            }
        }
        let token: UInt64
        if verifier == 0 {
            guard cookie == 0 else { throw POSIXError(.ESTALE) }
            if let existing = cursors.first(where: { $0.value.directory == directory.object }) {
                // Each request seeks to its caller's own cookie. Independent
                // listings can safely reuse this immutable namespace snapshot.
                token = existing.key
            } else {
                if cursors.count >= maximumCachedDirectorySnapshots,
                   let oldest = cursorOrder.first, let old = cursors[oldest] {
                    try pwCheck(pw_dir_close(volume, old.cursor))
                    cursors.removeValue(forKey: oldest)
                    cursorOrder.removeFirst()
                    // An evicted verifier explicitly returns ESTALE on resume.
                }
                var cursor: UInt64 = 0
                try pwCheck(pw_dir_begin(volume, directory.object, &cursor))
                token = cursor
                cursors[token] = (directory.object, cursor)
            }
        } else {
            token = verifier
        }
        guard let snapshot = cursors[token], snapshot.directory == directory.object else {
            throw POSIXError(.ESTALE)
        }
        cursorOrder.removeAll { $0 == token }
        cursorOrder.append(token)
        try pwCheck(pw_dir_seek(volume, snapshot.cursor, cookie))
        var position = cookie
        while true {
            var bytes = [UInt8](repeating: 0, count: 255)
            var count = 0
            var object: Int64 = 0
            let code = pw_dir_next(volume, snapshot.cursor, &bytes, bytes.count, &count, &object)
            if code == PW_END { break }
            try pwCheck(code)
            var stat = PwStat()
            try pwCheck(pw_stat(volume, object, &stat))
            let transient = PortaWeaveItem(owner: identity, object: object, kind: stat.kind)
            let values: FSItem.Attributes?
            if attributes != nil {
                let value = FSItem.Attributes()
                value.invalidateAllProperties()
                value.type = transient.itemType
                value.size = stat.size
                value.fileID = try transient.wireID()
                values = value
            } else { values = nil }
            guard try pack(FSFileName(data: Data(bytes.prefix(count))), transient.itemType,
                           transient.wireID(), position + 1, values) else { break }
            position += 1
        }
        return token
    }

    func refreshedCapacity() -> PwSpace {
        var fresh = PwSpace()
        let code = pw_volume_space(volume, &fresh)
        if code == PW_OK { capacity = fresh } else {
            record(POSIXError(.EIO))
            // Retain known logical quota but do not expose stale writable space.
            capacity.apparent_free_bytes = 0
            capacity.host_data_admission_bytes = 0
        }
        return capacity
    }
}
