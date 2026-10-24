// M0-I experimental FSKit App Extension entrypoint. Not installed/signed.
import Foundation
import FSKit

@main
struct PortaWeaveFSKitExtension: UnaryFileSystemExtension {
    var fileSystem: FSUnaryFileSystem & FSUnaryFileSystemOperations {
        PortaWeaveUnaryFileSystem()
    }
}
