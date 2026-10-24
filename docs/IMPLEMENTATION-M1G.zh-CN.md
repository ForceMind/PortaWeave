# M1-G 第一增量：后端存储同步屏障

新增 `Volume::synchronize_storage` 和同一原生 ABI 的 `pw_storage_synchronize`。Unix 路径真实同步所有跟踪数据段（包含旧读取租约保留的段）、segments 目录、SQLite FULL 检查点后的 catalog/WAL、格式文件和容器目录。使用流式 SQL 枚举，不另建全段 ID 数组；保持现有句柄、读取租约和业务内容不变，不触发 GC。

Windows/non-Unix 明确返回 UnsupportedOperation/PW_UNSUPPORTED，不以现有目录 helper 的 no-op 冒充同步。C ABI 失败不修改输出。忙碌检查点为 BUSY，不锁定写保护；真实 I/O/结构错误或注入边界失败会返回错误并写保护。重新打开仍按既有 verify/recovery 规则处理。

这不是完整文件系统 fsync、安全卸载或断电认证。平台适配器仍需先排空自身和内核待处理写入、正确解释同步 flags，再结合安全生命周期；目录祖先、macOS 更强设备刷新、Windows 目录持久化、USB/exFAT 和物理断电未验证。WinFsp Flush 和 FSKit resource load 继续拒绝。详见 ADR 0004。

测试包括旧范围租约与覆盖/删除/关闭、真实 SQLite 读事务导致 BUSY、两个屏障边界的错误保护与重开、缺失跟踪段拒绝、Windows 明确未支持，以及实际 C caller 的成功/失败输出约定。执行结果以对应准确 SHA 的三平台 CI 为准，不能把源代码存在写成 PASS。
