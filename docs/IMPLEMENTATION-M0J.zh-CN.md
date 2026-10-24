# M0-J 第一增量：macOS 宿主 App 与真实 ExtensionKit 构建

承接 M0-I 的 FSKit 源码，新增版本控制内的 Xcode 工程、共享 scheme、SwiftUI 宿主 App、两个独立 bundle identity 和各自 sandbox entitlements。扩展使用 ExtensionKit product type，嵌入 `PortaWeave.app/Contents/Extensions/PortaWeaveFSKit.appex`，不放到 Foundation PlugIns 目录。App 和 Extension 均链接同一个既有 `portaweave-native` Rust 静态库，不依赖开发目录里的 dylib。扩展固定返回同一个 unary filesystem 实例。

## 构建与证据

在已具备 Xcode/FSKit SDK 的隔离 macOS 开发环境，仓库根运行：

```sh
bash ci/build-macos-lab.sh
```

脚本按当前主机构建 arm64 或 x86_64 单架构实验包，固定 Cargo.lock 和 Rust 工具链；首次下载 target/依赖需要正常网络。默认部署目标 26.0 来自 FSPathURLResource 的实际 SDK 可用性，不是产品兼容声明。首轮 Xcode 26.6 / macOS 26.5 SDK 构建 run 40106385275 因原先的 15.4 target 使用了仅 macOS 26+ 可用的 FSPathURLResource 而失败；保留该记录并将部署目标修正为 26.0，不添加运行时绕过。Apple Silicon 使用 `aarch64-apple-darwin`；Intel 使用 `x86_64-apple-darwin`。先构建原生静态库，再执行完整 `xcodebuild build`，验证宿主与嵌入扩展的实际 Mach-O、架构、bundle identity、Rust ABI 符号及动态依赖，保存日志和 JSON。已有证据目录不覆盖。

CI 只构建 **unsigned lab bundle**，不安装、注册、启用扩展，不创建系统挂载点。`CODE_SIGNING_ALLOWED=NO` 仅用于编译与链接验证，不能把产物当作已授权安装包；产品签名、公证和系统授权仍未完成。工程保留 sandbox、hardened runtime 和 Xcode user script sandbox，未加入绕过文件权限的 entitlement。若在 Xcode 使用个人签名，应由授权开发者配置团队；本轮不访问任何用户本机或签名凭证。

## 尚未完成

`probe/load/unload` 继续明确返回 ENOTSUP。没有 `FSVolume.Operations`、权限和同步实现时，打包完成也不能开放工作文件。下一阶段必须实现真正 FSVolume 生命周期、目录/文件回调、稳定卷 UUID、安全 scoped-resource 所有权以及有效的权限、同步与缓存语义。Windows WinFsp 的 ACL/共享/硬链接接口问题也仍未解决。

实际安装/启用/挂载、Finder/Explorer 操作、真实 Codex 正常沙盒、卷内开发工具、同一 exFAT 设备往返仍为 NOT_RUN/BLOCKED。CI 新增完整 App+Extension 构建这一层证据，不取代挂载验收。测试结果以本轮 PR 的准确 SHA/Actions 为准，保留首次失败记录。

Apple 官方接口与扩展配置参考：
https://developer.apple.com/documentation/fskit/building-a-passthrough-file-system
https://developer.apple.com/documentation/fskit/unaryfilesystemextension
