# M0-G｜WinFsp 回调边界与容量/SQLite 检查点

日期：2026-10-24；实验性工程，不可作为移动盘存放唯一副本。

## 实际新增

1. Rust `Volume::capacity` 报告逻辑配额、逻辑已占、宿主可用快照、软水位后的可写预算、`min(逻辑剩余, 宿主预算)`。包括仍被打开的已删除文件；垃圾段不提前计为可用。**不是任何预分配或写入成功保证。**
2. `Volume::checkpoint_catalog` 使用 SQLite `PRAGMA wal_checkpoint(FULL)`，检查 busy、WAL 总帧和已复制帧。成功时只代表 catalog WAL 被 SQLite 成功检查点，**绝不能冒充完整文件系统 fsync / WinFsp Flush / FSKit synchronize**。底层 exFAT/USB 和 Windows 目录刷新仍有缺口。
3. 向既有 `portaweave-native` 增补 C ABI `pw_volume_space`、`pw_catalog_checkpoint`，保持原 ABI v1 其余函数不变；增加原生头文件声明和 Rust ABI 测试。
4. Windows 编译针对 **真实 WinFsp 2.1 头文件** 的回调契约。GitHub Actions 使用上游固定提交 `ddca7bd5481857a65ba552f643b8776fd070836f` 下载 SDK 头，不把 GPL 头文件拷贝到我们仓库，也不安装 WinFsp 驱动或触碰用户机器。测试用 C 构造模拟 `FSP_FILE_SYSTEM`，直接调用 `GetVolumeInfo`、`Read` 与拒绝回调，验证容量语义和 ABI 正确连接。
5. 无文件权限/ACL 系统前，`GetSecurityByName`、`Open` 返回 `STATUS_ACCESS_DENIED`；Flush 返回 `STATUS_NOT_SUPPORTED`；Write/Create 等尚未接线。**故意不能挂载为可用工作盘，不能从 CI 成功宣称真正 WinFsp 挂载已通过。** 即使 WinFsp 初始化仍须更完整的缓存、链接、共享、身份、通知、文件同步和生命周期实现。

## 实际验收与后续门禁

本次测试通过须以 PR 对应的 CI SHA 为证据，不能因为说明文档已存在就写 PASS。

仍需真实 Windows WinFsp Dispatcher/盘符测试，FSKit 真正扩展、macOS/Windows 物理 exFAT 交接、异常掉电，以及 Codex/Git/依赖安装/编译/执行/权限负向测试。WinFsp 官方硬链接缺口不变。当前 `pw_catalog_checkpoint` 不是 OS flush 的实现；完成真正持久化和平台权限前不能让普通程序误判安全。

参考：
- WinFsp v2.1 API: https://winfsp.dev/doc/WinFsp-API-winfsp.h/
- WinFsp tutorial: https://winfsp.dev/doc/WinFsp-Tutorial/
- SQLite FULL checkpoint: https://www.sqlite.org/pragma.html#pragma_wal_checkpoint
- exFAT differences: https://learn.microsoft.com/en-us/windows/win32/fileio/filesystem-functionality-comparison

严禁为了使测试变绿，关闭 Codex 沙盒、强行以管理员运行程序、绕过文件权限、复制硬链接或将用户项目隐藏转移到系统盘。
