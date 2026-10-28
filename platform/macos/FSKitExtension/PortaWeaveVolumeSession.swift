// Native adapter contract, not an installed/mounted filesystem. The unary
// loader remains closed until metadata, permissions and platform gates pass.
import Foundation
import Darwin
import FSKit
import OSLog
import PortaWeaveNative

func pwCheck(_ status: Int32) throws {
    guard status != PW_OK else { return }
    if status == PW_BAD_COOKIE {
        throw NSError(domain: NSPOSIXErrorDomain, code: FSError.Code.invalidDirectoryCookie.rawValue)
    }
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
    case PW_DENIED: code = .EACCES
    case PW_UNSUPPORTED: code = .ENOTSUP
    case PW_RESOURCE: code = .ENOMEM
    case PW_BUFFER_TOO_SMALL: code = .EOVERFLOW
    case PW_INVALID: code = .EINVAL
    case PW_STALE: code = .ESTALE
    default: code = .EIO
    }
    throw POSIXError(code)
}

final class PortaWeaveItem: FSItem {
    let owner: UUID
    private(set) var object: Int64
    let kind: UInt32
    var handle: UInt64?
    var directoryHandle: UInt64?
    var modes: FSVolume.OpenModes = []

    init(owner: UUID, object: Int64, kind: UInt32, handle: UInt64? = nil,
         directoryHandle: UInt64? = nil) {
        self.owner = owner
        self.object = object
        self.kind = kind
        self.handle = handle
        self.directoryHandle = directoryHandle
        super.init()
    }

    // Only a private pending creation is rebound, before it enters the cache.
    func adoptCreated(_ created: PwFsKitCreated) {
        object = created.object
        handle = created.file == 0 ? nil : created.file
    }

    func adoptDirectoryCreated(_ created: PwFsKitDirectoryCreated) {
        object = created.object
        directoryHandle = created.directory
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
    private(set) var persistentIdentity: UUID?
    private(set) var directoryLifetimes = false
    let rootItem: PortaWeaveItem
    let queue = DispatchQueue(label: "dev.portaweave.volume")
    private let queueKey = DispatchSpecificKey<UUID>()
    private let logger = Logger(subsystem: "dev.portaweave.fs", category: "Volume")
    private(set) var volume: UInt64 = 0
    private var items: [Int64: PortaWeaveItem] = [:]
    private var cursors: [UInt64: (directory: Int64, cursor: UInt64, dots: Bool)] = [:]
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
            var storedIdentity = PwVolumeIdentity()
            let identityStatus = pw_volume_identity(volume, &storedIdentity)
            if identityStatus == PW_OK {
                persistentIdentity = UUID(uuid: storedIdentity.uuid)
                directoryLifetimes = storedIdentity.capabilities == 3
            } else if identityStatus != PW_UNSUPPORTED {
                try pwCheck(identityStatus)
            }
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

    func prepareCreation(kind: UInt32) throws -> PortaWeaveItem {
        try admitItem()
        items.reserveCapacity(items.count + 1)
        return PortaWeaveItem(owner: identity, object: 0, kind: kind)
    }

    func publishCreation(_ item: PortaWeaveItem, created: PwFsKitCreated) {
        item.adoptCreated(created)
        items[created.object] = item
    }

    func publishDirectoryCreation(_ item: PortaWeaveItem, created: PwFsKitDirectoryCreated) {
        item.adoptDirectoryCreated(created)
        items[created.object] = item
    }

    func intern(_ object: Int64, kind: UInt32) throws -> PortaWeaveItem {
        if let found = items[object] { return found }
        try admitItem()
        var handle: UInt64 = 0
        var directoryHandle: UInt64 = 0
        if kind == 0 { try pwCheck(pw_file_open(volume, object, &handle)) }
        if kind == 1 && directoryLifetimes && object != pw_root_id() {
            try pwCheck(pw_directory_open(volume, object, &directoryHandle))
        }
        let item = PortaWeaveItem(
            owner: identity, object: object, kind: kind, handle: kind == 0 ? handle : nil,
            directoryHandle: directoryHandle == 0 ? nil : directoryHandle
        )
        items[object] = item
        return item
    }

    func lookup(_ name: FSFileName, in directory: FSItem) throws -> (PortaWeaveItem, FSFileName) {
        let parent = try item(directory)
        guard parent.kind == 1 else { throw POSIXError(.ENOTDIR) }
        if name.string == "." {
            let stat = try status(parent)
            guard parent.object == pw_root_id() || stat.links != 0 else { throw POSIXError(.ENOENT) }
            return (parent, FSFileName(string: "."))
        }
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
        supported.formUnion(metadataAttributes)
        guard request.wantedAttributes.subtracting(supported).isEmpty else {
            throw POSIXError(.ENOTSUP)
        }
        let stat = try status(item)
        return try storedAttributes(item, stat: stat)
    }

    private var metadataAttributes: FSItem.Attribute {
        persistentIdentity == nil ? [] : [.uid, .gid, .mode, .birthTime, .accessTime, .modifyTime, .changeTime]
    }

    private func storedAttributes(_ item: PortaWeaveItem, stat: PwStat) throws -> FSItem.Attributes {
        let result = FSItem.Attributes()
        result.invalidateAllProperties()
        result.type = item.itemType
        result.size = stat.size
        result.fileID = try item.wireID()
        if item.kind == 0 {
            guard let count = UInt32(exactly: stat.links) else { throw POSIXError(.EOVERFLOW) }
            result.linkCount = count
        }
        if persistentIdentity != nil {
            var metadata = PwMetadata()
            try pwCheck(pw_metadata_get(volume, item.object, &metadata))
            result.uid = metadata.uid
            result.gid = metadata.gid
            result.mode = metadata.mode
            result.birthTime = try sdkTime(metadata.created)
            result.accessTime = try sdkTime(metadata.accessed)
            result.modifyTime = try sdkTime(metadata.modified)
            result.changeTime = try sdkTime(metadata.changed)
        }
        return result
    }

    // Native projection already checked all counts/time_t conversions before
    // commit. Supported macOS targets and the native module are 64-bit.
    func committedAttributes(_ values: PwFsKitAttributes, wireID: FSItem.Identifier) -> FSItem.Attributes {
        let result = FSItem.Attributes()
        result.invalidateAllProperties()
        result.type = values.kind == 0 ? .file : .directory
        result.size = values.size
        result.fileID = wireID
        if values.kind == 0 { result.linkCount = values.links }
        if values.metadata_valid == 1 {
            let metadata = values.metadata
            result.uid = metadata.uid
            result.gid = metadata.gid
            result.mode = metadata.mode
            result.birthTime = timespec(tv_sec: Int(metadata.created.seconds), tv_nsec: Int(metadata.created.nanoseconds))
            result.accessTime = timespec(tv_sec: Int(metadata.accessed.seconds), tv_nsec: Int(metadata.accessed.nanoseconds))
            result.modifyTime = timespec(tv_sec: Int(metadata.modified.seconds), tv_nsec: Int(metadata.modified.nanoseconds))
            result.changeTime = timespec(tv_sec: Int(metadata.changed.seconds), tv_nsec: Int(metadata.changed.nanoseconds))
        }
        return result
    }

    // Integer conversion keeps negative seconds and nanoseconds exact.
    private func sdkTime(_ time: PwTimestamp) throws -> timespec {
        guard time.reserved == 0, time.nanoseconds < 1_000_000_000,
              let seconds = Int(exactly: time.seconds) else { throw POSIXError(.EOVERFLOW) }
        return timespec(tv_sec: seconds, tv_nsec: Int(time.nanoseconds))
    }

    func metadataUpdate(_ request: FSItem.SetAttributesRequest) throws -> PwMetadataUpdate {
        var update = PwMetadataUpdate()
        if request.isValid(.uid) { update.valid |= UInt64(PW_META_UID); update.uid = request.uid }
        if request.isValid(.gid) { update.valid |= UInt64(PW_META_GID); update.gid = request.gid }
        if request.isValid(.mode) {
            guard request.mode <= 0o7777 else { throw POSIXError(.EINVAL) }
            update.valid |= UInt64(PW_META_MODE); update.mode = request.mode
        }
        if request.isValid(.birthTime) { update.valid |= UInt64(PW_META_CREATED); update.created = try nativeTime(request.birthTime) }
        if request.isValid(.accessTime) { update.valid |= UInt64(PW_META_ACCESSED); update.accessed = try nativeTime(request.accessTime) }
        if request.isValid(.modifyTime) { update.valid |= UInt64(PW_META_MODIFIED); update.modified = try nativeTime(request.modifyTime) }
        guard update.valid == 0 || persistentIdentity != nil else { throw POSIXError(.ENOTSUP) }
        return update
    }

    private func nativeTime(_ time: timespec) throws -> PwTimestamp {
        guard let seconds = Int64(exactly: time.tv_sec),
              let nanos = UInt32(exactly: time.tv_nsec), nanos < 1_000_000_000 else { throw POSIXError(.EINVAL) }
        return PwTimestamp(seconds: seconds, nanoseconds: nanos, reserved: 0)
    }

    // Explicit laboratory creation attributes; no OS caller credential inference.
    func creationAttributes(_ request: FSItem.SetAttributesRequest, parent: PortaWeaveItem,
                            directory: Bool) throws -> PwCreateAttributes? {
        var supported: FSItem.Attribute = [.uid, .gid, .mode, .birthTime, .accessTime, .modifyTime]
        if !directory { supported.insert(.size) }
        let ownership: FSItem.Attribute = [.uid, .gid, .mode]
        let times: FSItem.Attribute = [.birthTime, .accessTime, .modifyTime]
        var hasAttributes = false
        for bit in 0..<18 {
            let attribute = FSItem.Attribute(rawValue: 1 << bit)
            if request.isValid(attribute) {
                guard supported.contains(attribute) else { throw POSIXError(.ENOTSUP) }
                if ownership.contains(attribute) { hasAttributes = true }
                if times.contains(attribute) && persistentIdentity == nil { throw POSIXError(.ENOTSUP) }
            }
        }
        guard persistentIdentity != nil else {
            guard !hasAttributes else { throw POSIXError(.ENOTSUP) }
            return nil
        }
        var inherited = PwMetadata()
        try pwCheck(pw_metadata_get(volume, parent.object, &inherited))
        let mode = request.isValid(.mode) ? request.mode : (directory ? 0o700 : 0o600)
        guard mode <= 0o7777 else { throw POSIXError(.EINVAL) }
        return PwCreateAttributes(uid: request.isValid(.uid) ? request.uid : inherited.uid,
                                  gid: request.isValid(.gid) ? request.gid : inherited.gid,
                                  mode: mode, reserved: 0)
    }

    func creationState(_ request: FSItem.SetAttributesRequest) throws -> PwCreateState {
        var state = PwCreateState()
        if request.isValid(.size) { state.valid |= UInt64(PW_CREATE_SIZE); state.size = request.size }
        if request.isValid(.birthTime) { state.valid |= UInt64(PW_CREATE_CREATED); state.created = try nativeTime(request.birthTime) }
        if request.isValid(.accessTime) { state.valid |= UInt64(PW_CREATE_ACCESSED); state.accessed = try nativeTime(request.accessTime) }
        if request.isValid(.modifyTime) { state.valid |= UInt64(PW_CREATE_MODIFIED); state.modified = try nativeTime(request.modifyTime) }
        return state
    }

    func invalidateEnumerations() {
        for value in cursors.values {
            let status = pw_fskit_directory_close(volume, value.cursor)
            if status != PW_OK { record(POSIXError(.EIO)) }
        }
        cursors.removeAll()
        cursorOrder.removeAll()
    }

    func reclaim(_ raw: FSItem) throws {
        let item = try self.item(raw)
        guard item.modes.isEmpty else { throw POSIXError(.EBUSY) }
        for (key, value) in Array(cursors) where value.directory == item.object {
            try pwCheck(pw_fskit_directory_close(volume, value.cursor))
            cursors.removeValue(forKey: key)
            cursorOrder.removeAll { $0 == key }
        }
        if let handle = item.handle {
            try pwCheck(pw_file_close(volume, handle))
            item.handle = nil
            noteHealth()
        }
        if let directory = item.directoryHandle {
            try pwCheck(pw_directory_close(volume, directory))
            item.directoryHandle = nil
            noteHealth()
        }
        items.removeValue(forKey: item.object)
    }

    func close() throws {
        try requireOpen()
        // File and v3 directory identities stay pinned until explicit reclaim.
        // The root needs no pin: it cannot be unlinked while its owner exists.
        guard items.values.allSatisfy({ $0.handle == nil && $0.directoryHandle == nil && $0.modes.isEmpty }) else {
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
            var allowed: FSItem.Attribute = [.type, .size, .fileID]
            allowed.formUnion(metadataAttributes)
            guard request.wantedAttributes.subtracting(allowed).isEmpty else {
                throw POSIXError(.ENOTSUP)
            }
        }
        let token: UInt64
        let dots = attributes == nil
        if verifier == 0 {
            guard cookie == 0 else { throw NSError(domain: NSPOSIXErrorDomain, code: FSError.Code.invalidDirectoryCookie.rawValue) }
            if let existing = cursors.first(where: { $0.value.directory == directory.object }) {
                // A NEW enumeration captures current names/attributes. Prepare
                // before retiring the old verifier, even at shared admission.
                var fresh: UInt64 = 0
                var status = pw_fskit_directory_refresh(volume, existing.value.cursor, dots ? 1 : 0, &fresh)
                while status == PW_RESOURCE,
                      let oldest = cursorOrder.first(where: { $0 != existing.key }), let old = cursors[oldest] {
                    try pwCheck(pw_fskit_directory_close(volume, old.cursor))
                    cursors.removeValue(forKey: oldest)
                    cursorOrder.removeAll { $0 == oldest }
                    status = pw_fskit_directory_refresh(volume, existing.value.cursor, dots ? 1 : 0, &fresh)
                }
                try pwCheck(status)
                cursors.removeValue(forKey: existing.key)
                cursorOrder.removeAll { $0 == existing.key }
                token = fresh
                cursors[token] = (directory.object, fresh, dots)
            } else {
                if cursors.count >= maximumCachedDirectorySnapshots,
                   let oldest = cursorOrder.first, let old = cursors[oldest] {
                    try pwCheck(pw_fskit_directory_close(volume, old.cursor))
                    cursors.removeValue(forKey: oldest)
                    cursorOrder.removeFirst()
                    // An evicted verifier explicitly returns ESTALE on resume.
                }
                var cursor: UInt64 = 0
                var status = pw_fskit_directory_begin(volume, directory.object, dots ? 1 : 0, &cursor)
                // Native Windows/admin snapshots now share cursor/entry admission.
                // Evict only this session's existing LRU snapshots and retry;
                // foreign typed snapshots and live file pins are never evicted.
                while status == PW_RESOURCE, let oldest = cursorOrder.first, let old = cursors[oldest] {
                    try pwCheck(pw_fskit_directory_close(volume, old.cursor))
                    cursors.removeValue(forKey: oldest)
                    cursorOrder.removeFirst()
                    status = pw_fskit_directory_begin(volume, directory.object, dots ? 1 : 0, &cursor)
                }
                try pwCheck(status)
                token = cursor
                cursors[token] = (directory.object, cursor, dots)
            }
        } else {
            token = verifier
        }
        guard let snapshot = cursors[token], snapshot.directory == directory.object, snapshot.dots == dots else {
            throw POSIXError(.ESTALE)
        }
        cursorOrder.removeAll { $0 == token }
        cursorOrder.append(token)
        try pwCheck(pw_fskit_directory_seek(volume, snapshot.cursor, cookie))
        var position = cookie
        while true {
            var bytes = [UInt8](repeating: 0, count: 255)
            var count = 0
            var object: Int64 = 0
            var captured = PwFsKitAttributes()
            let code = pw_fskit_directory_next(volume, snapshot.cursor, &bytes, bytes.count, &count, &object, &captured)
            if code == PW_END { break }
            try pwCheck(code)
            let transient = PortaWeaveItem(owner: identity, object: object, kind: captured.kind)
            let wireID = try transient.wireID()
            let values: FSItem.Attributes?
            if attributes != nil {
                values = committedAttributes(captured, wireID: wireID)
            } else { values = nil }
            guard try pack(FSFileName(data: Data(bytes.prefix(count))), transient.itemType,
                           wireID, position + 1, values) else { break }
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
