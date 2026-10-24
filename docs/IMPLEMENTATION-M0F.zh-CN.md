# M0-F：主线 Native Bridge 的 Windows C / macOS Swift 调用验证

日期：2026-10-24。基于已合并主线 M0-E（`portaweave-native`），不创建第二套 ABI，不改变格式。

## 目的

核验一个 native 文件系统适配器的两个常见调用入口：

- Windows：`cl.exe` 编译真实 C 语言程序，链接 Rust `portaweave-native.dll` 的导入库，按同一份 C 头文件执行创建、写入、读取、重命名、关闭、重新打开和对象查找。
- macOS：`swiftc` 导入现有 C 头文件的 Clang module，在 FSKit SDK 可用的环境中通过 Swift→C ABI→Rust→SQLite/DataSegments 执行文件和目录操作。**不是 FSKit app extension，也没有向 macOS 注册或挂载文件系统。**
- Linux、macOS：保留已有 C caller 回归，不改变既有 smoke 测试。

CI 使用全新 Runner Temp 目录，不访问物理硬盘，不安装 Windows 驱动，不修改用户的 Mac，不启动 Codex，也不打开外部端口。三平台代码一致，固定 Cargo.lock 和工具链。

## 验收与局限

三个环境均应报告非零错误即失败，不得通过“缺少编译器则跳过”把 Windows 列绿。Windows 动态获取 VS `vcvars64.bat`，不依赖容易变化的工作负载标签；本机制在 PR #8 先前的 MSVC Runner 已经验证过一次，但本次使用的是**另一个 native-bridge 头文件及链接库**，必须重新验收。

此次所证明的仅是语言/链接 ABI 与共享 Rust 容器接口。未实现/验证：
- 真正 FSKit ExtensionKit 签名/授权/启用、FSVolume 回调、Finder 挂载；
- 真正 WinFsp 驱动/文件系统回调、盘符、Win32 文件共享和安全描述符；
- 正确同步、内核缓存一致性、执行/mmap、符号链接/原生硬链接、xattr、通知、Codex 沙盒负向测试；
- exFAT 物理介质、USB 插拔或真实断电可靠性。

所有这些仍是 [Issue #1](https://github.com/ForceMind/PortaWeave/issues/1) 发布阻断条件。严格禁止用 C/Swift smoke 的成功宣称 PortaWeave 已是一块可以工作的跨平台磁盘。

### 文档/脚本

- `ci/run-native-abi-windows.ps1`：标准 Windows/MSVC 实际 C 调用；
- `platform/macos/NativeBridgeSmoke.swift`：Swift 和 FSKit SDK 导入，以及原生 C ABI 调用；
- `include/module.modulemap`：Clang 模块映射，原有 C API 头文件唯一权威，不另开一套。
