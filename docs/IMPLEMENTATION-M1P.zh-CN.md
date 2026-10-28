# M1-P：有界描述符协作范围锁

日期：2026-10-29。接续 M0-W，新增 Rust 核心和 trusted-policy C ABI 的协作锁原语，未挂载系统卷。见 [ADR 0018](adr/0018-cooperative-descriptor-range-grants.md)。准确 source/tree、三平台 CI 与最终 main 在对应 PR 留档。

`Volume::lock_file_range/unlock_file_range` 和 `PosixFile::lock_range/unlock_range` 绑定活的普通文件 handle 与 inode，不绑定名字。共享可重叠；不同 handle 的重叠范围只要有独占即 LockConflict；相邻范围允许。有限半开区间可越过当前EOF，0长度和u64溢出拒绝。同一handle的每次申请独立发token、独立释放，不隐含合并/升级/降级/部分解锁或到未来EOF。原始和policy共用实际4096项上限，不是堆内存精确预算。

Policy共享需要已授予READ、独占需要已授予WRITE；chmod/rename/unlink后仍按原grant操作，不重新查路径。只有原活handle可释放该token。close在维护前移除自己的所有锁，不释放另一个open的锁；存储保护下也可释放。锁本身不额外pin inode/数据；owner退出/重开没有持久锁，也不把token作为认证秘密。

新增 `pw_posix_file_lock_range/unlock_range`，typed表关联原policy file；输出/模式先验证、native token先保留、core成功后发布。冲突独立返回 `PW_LOCK_CONFLICT=-20`，原ABI值/格式/schema/能力位不变。拒绝不修改输出、不投毒health、不触发GC。volume BUSY包含锁，policy close清除自己native/core两层锁。typed/foreign/stale/wrong-file拒绝。

**协作锁不会阻拦普通read/write/truncate、目录操作、mmap或读取租约。** 不是完整fcntl/OFD/flock、Windows共享模式或LockFileEx；未实现等待/取消/死锁探测、范围转换/拆并/查询、OS进程/dup/fork生命周期，也未接入系统锁回调，不能据此宣布SQLite/Codex或权限验收通过。

验证：core新增6项all-feature/5项default，覆盖v1/v2/v3硬链接别名、边界/EOF、失败不变、同handle多grant、共享raw/policy真实4096预算、open权限保留、外卷/重开、8真实线程独占竞争及保护下释放。native新增3项，覆盖v2/v3、实际4096、跨类型/外卷/错误输出和close释放；integration累计56/unit3。初轮native fixture误用access=1/3被既有ABI拒绝，已改用正确READ=4/RW=6，不放宽生产参数校验；最终完整回归单独记录，不抹掉初轮日志。

实际C11 caller新增 NATIVE_ADVISORY_RANGE_LOCK_ABI_OK，并保留全部既有三格式测试；独立C++17 TU验证公共头/签名/链接。v2 artifact交接每站新增 ADVISORY_RANGE_LOCK_HANDOVER_OK，验证别名冲突、删除后close释放和最终GC；本地阶段名不算其他OS实测。三平台新源码CI、旧reader拒绝、完整macOS App/Extension及原始WinFsp/SDK合同须准确日志核验，不预报成功。

真实挂载仍BLOCKED/NOT_RUN；Windows缺driver/runtime，macOS签名/授权及安全接入未完成。原生身份/ACL、共享模式、系统锁、缓存/同步、硬/符号链接、xattr、映射/执行、真卷工具/Codex正常沙盒与越界限制、物理exFAT/USB/断电门禁保留。未操作用户本机、安装驱动、改变服务或降低沙盒。
