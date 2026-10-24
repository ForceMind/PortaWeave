// M0-F: standalone Swift -> Clang C header -> Rust cdylib smoke. This is
// NOT an FSKit extension, a real mount, or a sandbox/permission demonstration.
import Foundation
import FSKit
import PortaWeaveNative

func check(_ status: Int32, _ label: String) {
    guard status == 0 else {
        fputs("\(label) returned \(status)\n", stderr)
        exit(1)
    }
}

@available(macOS 15.4, *)
func fsKitSDKTypecheckMarker(_ resource: FSResource? = nil) -> Bool {
    return resource == nil
}

guard CommandLine.arguments.count == 2 else {
    fatalError("Expected a new, disposable container directory path")
}
let path = Array(CommandLine.arguments[1].utf8)
var volume: UInt64 = 0
check(path.withUnsafeBufferPointer {
    pw_volume_create($0.baseAddress, $0.count, 64 * 1024 * 1024, &volume)
}, "create")
let name = Array("swift-native.txt".utf8)
var fileID: Int64 = 0
check(name.withUnsafeBufferPointer {
    pw_file_create(volume, pw_root_id(), $0.baseAddress, $0.count, &fileID)
}, "file_create")

var handle: UInt64 = 0
check(pw_file_open(volume, fileID, &handle), "file_open")
let data = Array("Swift native bridge".utf8)
var written = 0
check(data.withUnsafeBufferPointer {
    pw_file_write(volume, handle, 0, $0.baseAddress, $0.count, &written)
}, "file_write")
guard written == data.count else { fatalError("short ABI write") }
var result = [UInt8](repeating: 0, count: 128)
var readCount = 0
check(result.withUnsafeMutableBufferPointer {
    pw_file_read(volume, handle, 0, $0.baseAddress, $0.count, &readCount)
}, "file_read")
guard Array(result.prefix(readCount)) == data else { fatalError("mismatched ABI content") }
guard pw_volume_close(volume) == -7 else { fatalError("must not close open file") }
check(pw_file_close(volume, handle), "file_close")

var cursor: UInt64 = 0
check(pw_dir_begin(volume, pw_root_id(), &cursor), "dir_begin")
var enumerated = [UInt8](repeating: 0, count: 256)
var enumeratedLength = 0
var enumeratedID: Int64 = 0
check(enumerated.withUnsafeMutableBufferPointer {
    pw_dir_next(volume, cursor, $0.baseAddress, $0.count, &enumeratedLength, &enumeratedID)
}, "dir_next")
guard enumeratedID == fileID,
      Array(enumerated.prefix(enumeratedLength)) == name else {
    fatalError("directory cursor did not preserve native Swift name")
}
check(pw_dir_close(volume, cursor), "dir_close")
check(pw_volume_close(volume), "volume_close")
check(path.withUnsafeBufferPointer {
    pw_volume_open($0.baseAddress, $0.count, &volume)
}, "reopen")
var lookupID: Int64 = 0
check(name.withUnsafeBufferPointer {
    pw_lookup(volume, pw_root_id(), $0.baseAddress, $0.count, &lookupID)
}, "lookup")
guard lookupID == fileID else { fatalError("file identity changed after reopen") }
check(pw_volume_close(volume), "final close")
print("NATIVE_SWIFT_C_ABI_OK (FSKit SDK imported; no mount or extension executed)")
