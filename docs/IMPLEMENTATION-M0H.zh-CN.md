# M0-H｜Windows WinFsp 生命周期服务（实验性）

## 本轮代码
`platform/windows/winfsp_service.c` 包含**真实的** WinFsp 2.1 `FspFileSystemCreate`、`SetMountPoint`、`StartDispatcher`、`StopDispatcher`、`Delete` 调用，并经既有 `portaweave-native` C ABI 持有 Rust/SQLite 独占容器。

仅有两个明确入口：

- `--self-test`：验证官方回调函数表；不加载 WinFsp 驱动、不启动服务、不挂载、不开容器。该入口是当前 Windows CI 唯一执行内容。
- `--lab-mount R:`：**明确手动触发**、仅允许当前未占用的 R:–Z:；程序始终在系统临时目录创建新的 128 MiB 逻辑配额实验卷；不接受指向已有文件的数据路径，不安装驱动。仅从 Program Files 的 WinFsp 安装目录按绝对路径加载 DLL，完成真正 Dispatcher/盘符启动，Ctrl+C 停止并释放 WinFsp 与容器独占锁。此入口尚未在真实已安装驱动的 Windows 环境运行，状态 **NOT_RUN**。

**限制与安全**：挂载模式故意拒绝正常用户的 Open / GetSecurityByName；Write/Create/ReadDirectory 尚未接线；Flush 返回 `STATUS_NOT_SUPPORTED`。因此不是可用工作盘，不得用于保存任何实际数据，更不能宣称 Codex / Git 已兼容。只有后续实现有效用户 ACL、Win32 共享/锁、正确文件同步和可执行映射，并在实际 WinFsp 挂载盘符内完成应用闭环测试，才能解除这一限制。内层 SQLite 检查点不是底层 exFAT、Windows 目录缓存或 USB 硬件的同步保证。

此外，macOS FSKit 的真正 App Extension 和授权挂载仍然 **NOT_RUN**。WinFsp 硬链接接口缺口属于发布阻断项，不能用内部 Rust 硬链接掩盖。

## CI 与许可
CI 仅从 WinFsp 官方 GitHub 固定 commit `ddca7bd5481857a65ba552f643b8776fd070836f` 检出头文件进行编译，不安装或上传 WinFsp 驱动，也不把第三方源码放进本仓库。是否真实通过以对应 PR 所载精确 SHA / Actions 为准，本文件不预称已通过。

后续 M0-N/R/T 已接入实验性的普通文件 I/O、基本时间设置和不可变目录 ReadDirectory 合同；本文件的未接线列表描述 M0-H 当时状态。当前目录 Open/当前 GFI/刷新及原生 ACL/共享/Flush/真实挂载仍未完成，见 IMPLEMENTATION-M0T；不能从新增函数表推断可用工作盘。

M0-X后续修复信号准备/等待错误/异常停止与事件生命周期；Ctrl+C/Break仅为实验协作停止，窗口关闭/注销/关机不作为安全卸载。见IMPLEMENTATION-M0X。
