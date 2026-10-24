// M0-I: compile-time FSKit unary-file-system contract, NOT an active
// filesystem implementation. Until effective file permissions, sync and
// FSVolume.Operations are implemented, the extension must fail closed.
import Foundation
import FSKit
import OSLog
import PortaWeaveNative

final class PortaWeaveUnaryFileSystem: FSUnaryFileSystem, FSUnaryFileSystemOperations {
    private let logger = Logger(subsystem: "dev.portaweave.fs", category: "FSKit")

    func probeResource(
        resource: FSResource,
        replyHandler: @escaping (FSProbeResult?, (any Error)?) -> Void
    ) {
        guard resource is FSPathURLResource else {
            replyHandler(nil, POSIXError(.EINVAL))
            return
        }
        // A matching directory name alone is NOT evidence that an on-disk
        // volume is valid or safe to expose. The format lacks stable UUIDs
        // and production recovery; do not claim this is usable to the OS.
        logger.notice("PortaWeave format probing is not implemented")
        replyHandler(nil, POSIXError(.ENOTSUP))
    }

    func loadResource(
        resource: FSResource,
        options: FSTaskOptions,
        replyHandler: @escaping (FSVolume?, (any Error)?) -> Void
    ) {
        _ = resource
        _ = options
        // Never return an FSVolume until mount/unmount/synchronize, data,
        // metadata and sandbox authorization semantics exist and are tested.
        logger.warning("Refusing to load an incomplete PortaWeave volume; core ABI \(pw_abi_version())")
        replyHandler(nil, POSIXError(.ENOTSUP))
    }

    func unloadResource(
        resource: FSResource,
        options: FSTaskOptions,
        replyHandler: @escaping ((any Error)?) -> Void
    ) {
        _ = resource
        _ = options
        // Nothing was loaded; don't silently report an unmount occurred.
        replyHandler(POSIXError(.ENOTSUP))
    }

    func didFinishLoading() {
        logger.debug("PortaWeave FSKit extension runtime entered")
    }
}
