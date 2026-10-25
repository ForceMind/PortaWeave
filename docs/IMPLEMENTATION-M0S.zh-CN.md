# M0-S：原生名字操作的事务内身份校验

日期：2026-10-26。继续 M0-P/Q/R，不改格式、schema、名称比较或依赖。范围为 Rust/C 的受信任管理请求及 FSKit 实验回调，不是原生授权或挂载验收。

## 解决的请求窗口

已有 FSKit remove/rename 先调用 lookup 比较缓存 item，再调用独立的原始 C 名字修改接口。Swift 队列会串行处理自身回调，但两次 C 调用之间没有一个覆盖身份检查和修改的原生请求契约。若其他合法 owner 请求替换该名字，原始按名字修改可能作用于另一个 inode。不是指另一个 Volume owner 获得容器锁，也不把原始 backing 目录篡改列为安全使用方式。

增加 `Volume::remove_checked` 和 `rename_checked(RenameExpectation)`，C 对应 `pw_remove_checked` / `pw_rename_checked`。在同一次独占 owner 请求中比较预期 source/target，并在实际 SQL 修改事务内再次检查。名字缺席或不再指向预期 inode，返回新的 `StaleEntry` / `PW_STALE=-18`；不修改名字、对象、父时间、数据或维护状态，也不触发 GC 或写保护。真正 commit 错误仍可能结果不明。

rename 的 target=None / C target=0 要求目标名字不存在，target=正 ID 要求该 inode 仍为替换目标。同目录同比较键的仅大小写/规范化改名允许 target=None，因为它是同一个目录项；两个名字即使指向同一硬链接 inode，仍是不同目录项，不能拿它绕过“目标应不存在”。显式匹配两个 alias 的既有 rename no-op 保留。

目录类型、非空、环路、原子替换和旧打开 inode 的既有规则继续生效。原始 unlink/rmdir/rename 共用相同实现，保持原调用方式；新增 BeforeCommit 注入覆盖删除/改名事务，包括大小写改名。业务提交后的维护错误仍返回已完成结果，健康/写保护独立报告，保护下可以释放 pin。

## FSKit 接线

remove/rename 直接把通过本 session tag 验证的 FSItem ID 和可选 overItem ID 交给新 C 接口，不再依赖一次额外 lookup/open/intern 来保护随后按名字操作。PW_STALE 映射 ESTALE。成功才更新健康状态和失效本地枚举；事务内无可失败的提交后身份查询。目录的 SDK busy 检查继续保留。

这些 ID 是特定卷内的对象编号，不是秘密、权限、Windows share mode 或目录项世代。相同 inode 被重新链接到同名时仍可匹配，不声称检测所有 ABA 历史。原始管理 C API 仍不可暴露给不可信调用者；本增量没有把 expected ID 转成 policy grant。FSKit access/probe/load/activate/mount 和 WinFsp security/Open/Flush 继续关闭。

## 验证

新增核心 all-feature 6/default 4 项：v1/v2 替换后 stale 拒绝、源/目标/缺席检查、大小写/Unicode、两个硬链接 alias、目录/环路、提交前回滚、提交后维护故障与 pin/重开。新增 native ABI 3 项，包括两个真实线程通过 Barrier 控制“lookup 已观察旧对象、另一个请求替换名字、checked 删除/rename 拒绝”的事件顺序，并核对全部元数据、旧/新内容和健康状态。不是 OS 多进程或沙盒测试。

真实 C caller 使用新 API 和 STALE；artifact 各阶段输出 `CHECKED_NAMESPACE_HANDOVER_OK`。macOS SDK 合同在 v1/v2 中直接测试已缓存 file/directory 过时、源/目标更换、目标意外出现/消失、仅大小写改名、硬链接 no-op、正确替换和 pin 回收，标记 `FSVOLUME_CHECKED_NAMESPACE_CONTRACT_OK`。精确通过状态与源码 SHA 以 VALIDATION/对应 PR 为准，不能从 Work Linux 预报 macOS SDK 结果。

真实系统挂载、原生 caller/ACL/共享、内核缓存/锁/监听/执行/mmap/同步/安全卸载、Codex 正常沙盒和 backing 目录越界拒绝、物理 exFAT/USB/断电继续 BLOCKED/NOT_RUN。没有安装驱动、启用扩展、改变用户机器或降低安全要求。
