import SwiftUI
import PortaWeaveNative

@main
struct PortaWeaveApp: App {
    var body: some Scene {
        WindowGroup {
            VStack(alignment: .leading, spacing: 18) {
                Label("PortaWeave · 随卷", systemImage: "externaldrive")
                    .font(.largeTitle)
                Text("Native filesystem laboratory")
                    .font(.title2)
                Text("Core ABI \(pw_abi_version()) · FSKit extension bundled")
                Text("System mounting is not available in this build. Volume operations, permissions and durable synchronization are still under development.")
                Text("此构建用于验证原生打包，尚不能挂载或保存工作文件。签名、系统授权及真实挂载验收尚未完成。")
                    .foregroundStyle(.secondary)
                Link("Project and validation records", destination: URL(string: "https://github.com/ForceMind/PortaWeave")!)
            }
            .padding(32)
            .frame(minWidth: 560, maxWidth: 680)
            .textSelection(.enabled)
        }
    }
}
