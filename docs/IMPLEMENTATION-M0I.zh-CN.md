# M0-I｜macOS FSKit App Extension 接入契约（实验性）

## 代码范围

根据 Apple 官方 FSKit UnaryFileSystemExtension / FSUnaryFileSystem / FSUnaryFileSystemOperations 接口，新增：
- `platform/macos/FSKitExtension/PortaWeaveFSKitExtension.swift`：带 `@main` 的真实 FSKit Extension 入口类型（源代码）。
- `PortaWeaveUnaryFileSystem.swift`：`FSUnaryFileSystem` 子类，实现 unary probe/load/unload/didFinishLoading 契约。
- `Info.plist`：FSKit personality、短名、PathURL 资源、security-scoped 路径要求。
- `PortaWeaveFSKitExtension.entitlements`：FSKit 模块 entitlement + sandbox。

**这只是可编译的扩展源代码与元数据，不是已经打包的 Xcode App Extension 或可以安装的 FSKit 产品。** 此版本缺少宿主 macOS App target、签名与 entitlements 授权、Finder mount 生命周期和 `FSVolume` 实现。CI 仅对源文件在 macOS FSKit SDK 下执行 Swift type-check、对 plist 执行 `plutil -lint`。

## 为什么明确拒绝挂载

`probeResource` 会要求 FSPathURLResource 类型，但仍返回 ENOTSUP；`loadResource` 也始终返回 ENOTSUP；`unloadResource` 不伪报已卸载。

原因：现有容器尚未实现完整 FSVolume.Operations、正确的 synchronize/fsync、权限/xattr/符号链接及 Unix mode/执行、热插拔和挂载恢复，格式中也还没有用于 FSKit 的持久化卷 UUID。直接返回“usable”或创建只有演示功能的虚假 FSVolume，会导致 Finder、IDE 或 Codex 以为可以安全保存、编译和同步。

## 下一步必须完成

1. 正式宿主 App + ExtensionKit target（打包、签名、系统扩展启用与授权）；只在授权的临时测试路径操作。
2. 从 FSPathURLResource 获取 security-scoped URL、检查实际容器格式与卷 UUID；对权限不足、坏格式和活动 owner 明确报错。
3. 实现 FSVolume.Operations 必需的 activate/deactivate/mount/unmount/synchronize/lookup/enumerate/create/remove/rename，支持 FSVolume.ReadWriteOperations 和文件句柄生命周期；所有方法调用共享 Rust C ABI。
4. 正确处理 file ID、时间戳、权限、执行位、xattr、cache invalidation、沙盒访问和 flush。没有实现不能回报成功。
5. 在专用已授权 macOS 测试环境中完成**真正 mount + Finder 操作 + IDE/Git/Codex**，不能把 Swift type-check 当成真挂载测试。

目前 WinFsp 和 FSKit 的实际挂载仍为 **NOT_RUN/BLOCKED**。任何成功声明须附具体的 OS build、签名身份、完整日志与实测挂载信息，用户本机未经单独授权不得安装或启用扩展。

参考 Apple 官方 FSKit 文档：
- https://developer.apple.com/documentation/FSKit/building-a-passthrough-file-system
- https://developer.apple.com/documentation/fskit/fsunaryfilesystem
