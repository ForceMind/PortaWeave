// Real FSVolume callback implementation exercised directly with the SDK.
// No unary load path returns this volume until all platform gates are met.
import Foundation
import FSKit
import PortaWeaveNative

final class PortaWeaveVolume: FSVolume, FSVolume.Operations,
    FSVolume.OpenCloseOperations, FSVolume.ReadWriteOperations, FSVolume.AccessCheckOperations {
    let session: PortaWeaveVolumeSession

    init(laboratoryContainerURL url: URL) throws {
        let session = try PortaWeaveVolumeSession(laboratoryContainerURL: url)
        self.session = session
        // v2 has a persistent UUID; v1 retains an explicitly session-only ID.
        // The separate random session.identity still rejects stale FSItems.
        super.init(volumeID: FSVolume.Identifier(uuid: session.persistentIdentity ?? session.identity),
                   volumeName: FSFileName(string: "PortaWeave Laboratory"))
    }

    var maximumLinkCount: Int { Int(UInt32.max) }
    var maximumNameLength: Int { 255 }
    var restrictsOwnershipChanges: Bool { true }
    var truncatesLongNames: Bool { false }
    var maximumFileSize: UInt64 { UInt64(Int64.max) }
    var maximumXattrSize: Int { 0 }

    var supportedVolumeCapabilities: FSVolume.SupportedCapabilities {
        let value = FSVolume.SupportedCapabilities()
        value.supportsHardLinks = true
        value.supportsSparseFiles = true
        value.supports64BitObjectIDs = true
        value.supportsPersistentObjectIDs = session.persistentIdentity != nil
        value.caseFormat = .insensitiveCasePreserving
        value.doesNotSupportSettingFilePermissions = session.persistentIdentity == nil
        value.doesNotSupportRootTimes = session.persistentIdentity == nil
        return value
    }

    var volumeStatistics: FSStatFSResult {
        session.synchronized {
            let capacity = session.refreshedCapacity()
            let value = FSStatFSResult(fileSystemTypeName: "PortaWeaveLab")
            value.blockSize = 65_536
            value.ioSize = 16 * 1024 * 1024
            value.totalBytes = capacity.logical_quota_bytes
            value.usedBytes = capacity.logical_used_bytes
            value.availableBytes = capacity.apparent_free_bytes
            value.freeBytes = capacity.apparent_free_bytes
            value.totalBlocks = (capacity.logical_quota_bytes + 65_535) / 65_536
            value.usedBlocks = (capacity.logical_used_bytes + 65_535) / 65_536
            value.availableBlocks = capacity.apparent_free_bytes / 65_536
            value.freeBlocks = value.availableBlocks
            return value
        }
    }

    func activate(options: FSTaskOptions, replyHandler: @escaping (FSItem?, Error?) -> Void) {
        replyHandler(nil, POSIXError(.ENOTSUP))
    }
    func mount(options: FSTaskOptions, replyHandler: @escaping (Error?) -> Void) {
        replyHandler(POSIXError(.ENOTSUP))
    }
    func unmount(replyHandler: @escaping () -> Void) {
        // This protocol supplies no error channel. Acknowledge the notification
        // only; do not close an owner or declare safe eject from this callback.
        session.perform { replyHandler() }
    }
    func deactivate(options: FSDeactivateOptions, replyHandler: @escaping (Error?) -> Void) {
        session.perform {
            do { try self.session.close(); replyHandler(nil) }
            catch { self.session.record(error); replyHandler(error) }
        }
    }
    func synchronize(flags: FSSyncFlags, replyHandler: @escaping (Error?) -> Void) {
        session.perform {
            do {
                try self.session.requireOpen()
                guard flags == .wait || flags == .dWait else { throw POSIXError(.ENOTSUP) }
                var result = PwStorageSync()
                try pwCheck(pw_storage_synchronize(self.session.volume, &result))
                replyHandler(nil)
            } catch { self.session.record(error); replyHandler(error) }
        }
    }
    func getAttributes(
        _ desired: FSItem.GetAttributesRequest, of item: FSItem,
        replyHandler: @escaping (FSItem.Attributes?, Error?) -> Void
    ) {
        session.perform {
            do { replyHandler(try self.session.attributes(desired, of: item), nil) }
            catch { replyHandler(nil, error) }
        }
    }
    func setAttributes(
        _ attributes: FSItem.SetAttributesRequest, on raw: FSItem,
        replyHandler: @escaping (FSItem.Attributes?, Error?) -> Void
    ) {
        session.perform {
            do {
                let item = try self.session.item(raw)
                // Reject every unsupported field before any data mutation.
                let unsupported: [FSItem.Attribute] = [
                    .type, .linkCount, .flags, .allocSize, .fileID,
                    .parentID, .changeTime,
                    .backupTime, .addedTime, .supportsLimitedXAttrs, .inhibitKernelOffloadedIO
                ]
                guard !unsupported.contains(where: { attributes.isValid($0) }) else {
                    throw POSIXError(.ENOTSUP)
                }
                // Validate the SDK identifier before mutation. Native projection
                // validates all returned counts/times before business commit.
                let wireID = try item.wireID()
                var update = try self.session.metadataUpdate(attributes)
                var committed = PwFsKitAttributes()
                if attributes.isValid(.size) {
                    let handle = try self.session.fileHandle(item, requiring: .write)
                    try pwCheck(pw_fskit_file_set_attributes(
                        self.session.volume, handle, item.object, attributes.size, &update, &committed))
                } else {
                    try pwCheck(pw_fskit_metadata_set(
                        self.session.volume, item.object, &update, &committed))
                }
                // No database read or fallible conversion follows known success.
                let result = self.session.committedAttributes(committed, wireID: wireID)
                if attributes.isValid(.size) { attributes.consumedAttributes.insert(.size) }
                for attribute in [FSItem.Attribute.uid, .gid, .mode, .birthTime, .accessTime, .modifyTime] {
                    if attributes.isValid(attribute) { attributes.consumedAttributes.insert(attribute) }
                }
                self.session.noteHealth()
                replyHandler(result, nil)
            } catch { replyHandler(nil, error) }
        }
    }
    func lookupItem(
        named name: FSFileName, inDirectory directory: FSItem,
        replyHandler: @escaping (FSItem?, FSFileName?, Error?) -> Void
    ) {
        session.perform {
            do {
                let (item, canonical) = try self.session.lookup(name, in: directory)
                replyHandler(item, canonical, nil)
            } catch { replyHandler(nil, nil, error) }
        }
    }
    func reclaimItem(_ item: FSItem, replyHandler: @escaping (Error?) -> Void) {
        session.perform {
            do { try self.session.reclaim(item); replyHandler(nil) }
            catch { replyHandler(error) }
        }
    }
    func createItem(
        named name: FSFileName, type: FSItem.ItemType, inDirectory directory: FSItem,
        attributes: FSItem.SetAttributesRequest,
        replyHandler: @escaping (FSItem?, FSFileName?, Error?) -> Void
    ) {
        session.perform {
            do {
                let parent = try self.session.item(directory)
                guard type == .file || type == .directory else { throw POSIXError(.ENOTSUP) }
                let creation = try self.session.creationAttributes(attributes, parent: parent, directory: type == .directory)
                try self.session.admitItem()
                let data = try self.session.data(name)
                var object: Int64 = 0
                let status = data.withUnsafeBytes { bytes in
                    let pointer = bytes.bindMemory(to: UInt8.self).baseAddress
                    if var values = creation {
                        return pw_node_create_with_metadata(self.session.volume, parent.object, pointer,
                                                            data.count, type == .directory ? 1 : 0, &values, &object)
                    }
                    return type == .file ?
                        pw_file_create(self.session.volume, parent.object, pointer, data.count, &object) :
                        pw_directory_create(self.session.volume, parent.object, pointer, data.count, &object)
                }
                try pwCheck(status)
                for attribute in [FSItem.Attribute.uid, .gid, .mode] {
                    if attributes.isValid(attribute) { attributes.consumedAttributes.insert(attribute) }
                }
                self.session.noteHealth()
                self.session.invalidateEnumerations()
                replyHandler(try self.session.intern(object, kind: type == .file ? 0 : 1), name, nil)
            } catch { replyHandler(nil, nil, error) }
        }
    }
    func createLink(
        to raw: FSItem, named name: FSFileName, inDirectory directory: FSItem,
        replyHandler: @escaping (FSFileName?, Error?) -> Void
    ) {
        session.perform {
            do {
                let item = try self.session.item(raw)
                let parent = try self.session.item(directory)
                let bytes = try self.session.data(name)
                let status = bytes.withUnsafeBytes {
                    pw_hard_link(self.session.volume, item.object, parent.object,
                                 $0.bindMemory(to: UInt8.self).baseAddress, bytes.count)
                }
                try pwCheck(status)
                self.session.noteHealth()
                self.session.invalidateEnumerations()
                replyHandler(name, nil)
            } catch { replyHandler(nil, error) }
        }
    }
    func removeItem(
        _ raw: FSItem, named name: FSFileName, fromDirectory directory: FSItem,
        replyHandler: @escaping (Error?) -> Void
    ) {
        session.perform {
            do {
                let item = try self.session.item(raw)
                let parent = try self.session.item(directory)
                let (actual, _) = try self.session.lookup(name, in: parent)
                guard actual === item else { throw POSIXError(.ESTALE) }
                if item.kind == 1 && !item.modes.isEmpty { throw POSIXError(.EBUSY) }
                let bytes = try self.session.data(name)
                let status = bytes.withUnsafeBytes {
                    pw_remove(self.session.volume, parent.object, $0.bindMemory(to: UInt8.self).baseAddress,
                              bytes.count, item.kind == 1 ? 1 : 0)
                }
                try pwCheck(status)
                self.session.noteHealth()
                self.session.invalidateEnumerations()
                replyHandler(nil)
            } catch { replyHandler(error) }
        }
    }
    func renameItem(
        _ raw: FSItem, inDirectory sourceDirectory: FSItem, named sourceName: FSFileName,
        to destinationName: FSFileName, inDirectory destinationDirectory: FSItem, overItem: FSItem?,
        replyHandler: @escaping (FSFileName?, Error?) -> Void
    ) {
        session.perform {
            do {
                let item = try self.session.item(raw)
                let source = try self.session.item(sourceDirectory)
                let destination = try self.session.item(destinationDirectory)
                let (found, _) = try self.session.lookup(sourceName, in: source)
                guard found === item else { throw POSIXError(.ESTALE) }
                if let overItem = overItem {
                    let expected = try self.session.item(overItem)
                    if expected.kind == 1 && !expected.modes.isEmpty { throw POSIXError(.EBUSY) }
                    let (actual, _) = try self.session.lookup(destinationName, in: destination)
                    guard expected === actual else { throw POSIXError(.ESTALE) }
                }
                let a = try self.session.data(sourceName)
                let b = try self.session.data(destinationName)
                let status = a.withUnsafeBytes { old in b.withUnsafeBytes { new in
                    pw_rename(self.session.volume, source.object, old.bindMemory(to: UInt8.self).baseAddress,
                              a.count, destination.object, new.bindMemory(to: UInt8.self).baseAddress,
                              b.count, overItem == nil ? 0 : 1)
                } }
                try pwCheck(status)
                self.session.noteHealth()
                self.session.invalidateEnumerations()
                replyHandler(destinationName, nil)
            } catch { replyHandler(nil, error) }
        }
    }
    func enumerateDirectory(
        _ directory: FSItem, startingAt cookie: FSDirectoryCookie, verifier: FSDirectoryVerifier,
        attributes: FSItem.GetAttributesRequest?, packer: FSDirectoryEntryPacker,
        replyHandler: @escaping (FSDirectoryVerifier, Error?) -> Void
    ) {
        session.perform {
            do {
                let token = try self.session.enumerate(directory, cookie: cookie.rawValue,
                                                       verifier: verifier.rawValue, attributes: attributes) {
                    name, type, id, next, values in
                    packer.packEntry(name: name, itemType: type, itemID: id,
                                     nextCookie: FSDirectoryCookie(rawValue: next), attributes: values)
                }
                replyHandler(FSDirectoryVerifier(rawValue: token), nil)
            } catch { replyHandler(verifier, error) }
        }
    }
    func openItem(
        _ raw: FSItem, modes: FSVolume.OpenModes, replyHandler: @escaping (Error?) -> Void
    ) {
        session.perform {
            do {
                let item = try self.session.item(raw)
                guard !modes.isEmpty, modes.subtracting([.read, .write]).isEmpty else {
                    throw POSIXError(.EINVAL)
                }
                if item.kind == 1 && modes.contains(.write) { throw POSIXError(.EISDIR) }
                item.modes.formUnion(modes)
                replyHandler(nil)
            } catch { replyHandler(error) }
        }
    }
    func closeItem(
        _ raw: FSItem, modes: FSVolume.OpenModes, replyHandler: @escaping (Error?) -> Void
    ) {
        session.perform {
            do {
                let item = try self.session.item(raw)
                guard modes.subtracting(item.modes).isEmpty else { throw POSIXError(.EINVAL) }
                item.modes = modes
                replyHandler(nil)
            } catch { replyHandler(error) }
        }
    }
    func read(
        from raw: FSItem, at offset: off_t, length: Int, into buffer: FSMutableFileDataBuffer,
        replyHandler: @escaping (Int, Error?) -> Void
    ) {
        session.perform {
            do {
                guard offset >= 0, length >= 0, length <= 16 * 1024 * 1024,
                      length <= buffer.length else { throw POSIXError(.EINVAL) }
                let handle = try self.session.fileHandle(raw, requiring: .read)
                var count = 0
                let code = buffer.withUnsafeMutableBytes { bytes in
                    pw_file_read(self.session.volume, handle, UInt64(offset),
                                 bytes.bindMemory(to: UInt8.self).baseAddress, length, &count)
                }
                try pwCheck(code)
                replyHandler(count, nil)
            } catch { replyHandler(0, error) }
        }
    }
    func write(
        contents: Data, to raw: FSItem, at offset: off_t,
        replyHandler: @escaping (Int, Error?) -> Void
    ) {
        session.perform {
            do {
                guard offset >= 0, contents.count <= 16 * 1024 * 1024 else { throw POSIXError(.EINVAL) }
                let handle = try self.session.fileHandle(raw, requiring: .write)
                var count = 0
                let code = contents.withUnsafeBytes { bytes in
                    pw_file_write(self.session.volume, handle, UInt64(offset),
                                  bytes.bindMemory(to: UInt8.self).baseAddress, contents.count, &count)
                }
                try pwCheck(code)
                self.session.noteHealth()
                replyHandler(count, nil)
            } catch { replyHandler(0, error) }
        }
    }

    // No caller identity or persistent permission model exists in v1.
    // Normal activation and access must remain denied despite direct tests.
    func checkAccess(
        to item: FSItem, requestedAccess access: FSVolume.AccessMask,
        replyHandler: @escaping (Bool, Error?) -> Void
    ) { replyHandler(false, POSIXError(.EACCES)) }

    func readSymbolicLink(_ item: FSItem, replyHandler: @escaping (FSFileName?, Error?) -> Void) {
        replyHandler(nil, POSIXError(.ENOTSUP))
    }
    func createSymbolicLink(
        named name: FSFileName, inDirectory directory: FSItem,
        attributes: FSItem.SetAttributesRequest, linkContents: FSFileName,
        replyHandler: @escaping (FSItem?, FSFileName?, Error?) -> Void
    ) { replyHandler(nil, nil, POSIXError(.ENOTSUP)) }
}
