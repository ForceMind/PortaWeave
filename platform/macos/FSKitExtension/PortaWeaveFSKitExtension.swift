// M0-I experimental FSKit App Extension entrypoint. Not installed/signed.
import Foundation
import FSKit

@main
struct PortaWeaveFSKitExtension: UnaryFileSystemExtension {
    private let instance = PortaWeaveUnaryFileSystem()

    var fileSystem: FSUnaryFileSystem & FSUnaryFileSystemOperations {
        instance
    }
}
