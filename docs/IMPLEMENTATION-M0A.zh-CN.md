# M0-A 原生系统适配桥接层（实验性）

日期：2026-10-24；对应 Issue #1。目标是为 macOS FSKit 和 Windows 候选适配器提供可审计的同一入口，**不是已经挂载磁盘**。

## 已加入源代码

- `crates/ffi`：Rust `cdylib` / `rlib`，头文件 `include/portaweave.h`。
- 进程内递增 u64 会话编号；不暴露 Rust/SQLite 指针。卷有且只有一个持锁 owner；同一卷 I/O 通过 Mutex 串行执行。close 移除会话编号、拒绝后续请求，并等待已经执行中的请求。
- 稳定数字文件 ID 和句柄编号，受控地映射核心 `FileId` / `Handle`。句柄在关闭后失效，不重复分配。
- 核心已存在的创建卷、打开卷、查询、文件/目录建立、按范围读写、截断、重命名/替换、硬链接、删除、查询基本 Stat，以及维护失败状态读取。
- 各错误返回稳定的 `PW_*` 代码。回调中的 Rust panic 不越过 C ABI，而是返回 `PW_INTERNAL`。主存储核心依然 `forbid(unsafe_code)`，C 指针范围处理集中于单独的 FFI crate。

## 未提供且不可宣称

- **未提供 fsync/flush 回调。** 现在不能把 SQLite 上某次成功的业务操作冒充所有 OS 缓存已安全同步。内核必须先完成严格同步语义后才能接入正式文件系统。
- **未提供目录枚举、真实文件属性/时间、xattr/ADS、Unix mode、Windows ACL、符号链接、重解析点、文件共享/锁、目录通知、mmap 生命周期、卷容量配额接口。** `pw_stat` 只含内部基础属性，不能当作原生完整 stat。
- 已有硬链接操作在 Rust 容器中是真共享身份；WinFsp 官方硬链接缺口仍是平台门禁，不因 FFI 暴露 `pw_hard_link` 就解决。
- FFI 返回码不是 POSIX errno 或 NTSTATUS。平台映射层必须显式完成访问权限、锁和错误语义转换，不得回传假成功。
- 目前 C 字符串路径需要有效 UTF-8，非 UTF-8 Mac 路径、Windows UTF-16 特殊行为和跨平台名称比较仍须单独适配。
- 用户或 OS 直接修改底层 exFAT 目录会破坏假设，内部 ACL 无法阻止可访问 backing 的进程越界；正式 Codex 沙盒验证必须对原始 backing 目录做隔离测试。
- 采用进程本地 session IDs 而非跨进程服务；卷服务重启会更换 ID，外部不能持久保存它们。close 后任何旧 ID 必须失败。

## 平台下一步

1. macOS 建立真正可安装的 FSKit App Extension，使用 Apple 官方的 Unary File System / FSVolume 生命周期；采用签名与用户授权的文件访问，不通过临时 web 服务或 Mac kext 伪装挂载。FSKit API 文档链接：https://developer.apple.com/documentation/fskit/building-a-passthrough-file-system
2. Windows 评估 WinFsp native 或 Dokany 两条路线，逐项验证硬链接、执行、内存映射、ACL、文件通知和普通 Codex 本地沙盒；不能单纯因为出盘符就关闭 Issue #1。文档：https://winfsp.dev/doc/NTFS-Compatibility/
3. 文件系统适配器每个调用必须把本机用户/令牌和共享/权限上下文带入，不能绕过正常沙盒，也不能将整个容器开放给所有用户。
4. 增加真实 OS 挂载工作盘端到端检查：Git/pnpm/Cargo/venv/SQLite/IDE/Codex 在卷内实际运行；继续明确区分 CI 在宿主目录的 FFI 测试与 mounted .pwvol 测试。

## 证据级别

当 CI 通过时，只说明 C ABI 在对应操作系统普通测试目录上成功编译和执行相应用例。它**不能证明 FSKit/WinFsp 能挂载，不能证明 Codex、跨系统 exFAT、设备拔出或断电安全**。所有未完成发布门禁留在 Issue #1。
