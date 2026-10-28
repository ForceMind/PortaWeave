# M0-W：FSKit 名字/属性一致的目录枚举

日期：2026-10-28。接续 M0-V，修复实际 FSVolume session 的枚举缓存行为；**未安装或挂载系统文件卷**。架构见 [ADR 0017](adr/0017-fskit-captured-directory-enumeration.md)，准确 source/tree、平台 CI 与最终 main 见对应 PR。

## 问题与行为

旧适配器用 `pw_dir_begin` 保留名字/ID，打包条目时才逐次 `pw_stat`/`pw_metadata_get`。文件在两次分页之间被覆盖或删除回收，会出现旧名字配新属性，或分页中途 NotFound。旧的 initial verifier=0 还会重复使用原名字快照，无法明确区分新观察与旧页重放。

新增核心 administrative `directory_object_snapshot`，一次 owner 观察捕获目录、可选逻辑父目录、全部有序名字和 ObjectInfo；先校验条目上限。v1 metadata=None，v2/v3 保存真实属性，根的父目录为自己，v3 空孤儿没有父/子项。这是元数据副本，不保留 inode/数据块，也不授予路径或操作权限。

新增 typed C `pw_fskit_directory_begin/refresh/next/seek/close`，发布 token 前完成所有字段投影。next 不再查当前 catalog，文件被修改/删除/最终 GC 后仍重放当时的名字、身份、大小、链接数和支持的属性。refresh 只针对原 captured ID，绝不跟随原名字里的替代对象。失败/EOF/小缓冲都不修改输出或推进位置；seek 越界返回新增 PW_BAD_COOKIE=-19，错误类型/foreign/stale token 仍 INVALID。既有 ABI 数值/入口、旧 cursor、schema/marker/capabilities 不变，不迁移格式。

| SDK 请求 | 当前行为 |
|---|---|
| verifier=0、cookie=0 | 新捕获；已有自有快照时原子替换并返回新 verifier |
| 非零 verifier（包括 cookie=0） | 同一快照重放，不自动刷新 |
| 成功重启 | 旧 verifier ESTALE，旧 cookies 必须丢弃 |
| 重启失败 | 原 verifier、位置和捕获内容保留；不会按原名改读另一个目录 |
| packer 拒绝一行 | 返回最后成功打包的 cookie，下一次 seek 后重试该行 |
| 非法 cookie | NSPOSIXErrorDomain / FSError.Code.invalidDirectoryCookie |
| attributes=nil | 连接中的目录/root 包含 `.`、`..`；根两行都指向根 |
| attributes 非 nil | 不打包 dots；不能拿另一种 dot 模式的 verifier 继续 ordinal |
| 已删除的 v3 空目录 | 没有虚构父目录或 dots，保持空结果 |

依据固定 macOS26 的 [Operations 枚举文档](https://developer.apple.com/documentation/fskit/fsvolume/operations/enumeratedirectory%28_%3Astartingat%3Averifier%3Aattributes%3Apacker%3Areplyhandler%3A%29)，没有混入 macOS27 Handler/FSContext。直接 SDK 对象和 pack closure 只验证适配算法，真正 FSDirectoryEntryPacker/内核分发仍 NOT_RUN。

## 预算与生命周期

FSKit、Windows 与旧 administrative 三种 cursor 分表，共用 64 个 cursor 和 100000 retained entries，所有 dots 都计入。重建只抵扣自己的旧 slot/条目；完全准备并保留新单调 token 后才退役旧 token。旧/新副本会暂时共存，不是精确堆/峰值内存或 OOM 保护。

SDK 仍只保留 32 个自有 LRU 元数据快照。压力下只释放自己的其他快照，不关闭正在替换的目标，不驱逐 foreign cursor 或 file/directory pin。reclaim/会话关闭释放的是该 SDK 自有 typed cursor；活文件/目录 pin 和 BUSY 规则不变。WinFsp marker 重放、104-byte/8-byte wire、权限重新授权和单独显式刷新均保持原契约。

## 实测和待核验

Work Linux full default/all-feature workspace、clippy -D warnings、fmt、Python9、真实 C11 caller 和独立 C++17 公共头/类型/链接调用通过。新 core object_directory_snapshots all-feature3/default2 项；新 native fskit_directory 6 项（integration累计53，unit仍3）。既有降低到3-entry的真实共用预算单元加入第三种 cursor、dots、抵扣、增长拒绝和旧副本保留，未虚增 unit 数量，也不冒称实际100000名字压力测试。

新用例覆盖 v1无伪造元数据、v2/v3字段、覆盖/unlink/最后关闭后GC且旧行不变、逻辑父/root/空孤儿、错误输出/小缓冲/EOF/非法cookie、foreign/typed/stale token、64真实混合cursor下原子替换和4真实读线程与替换竞争。C caller 在三种格式输出 NATIVE_CAPTURED_DIRECTORY_ABI_OK；本地 v3 交接每站增加 OBJECT_DIRECTORY_SNAPSHOT_HANDOVER_OK。这些本地阶段全在Linux，不计作其他OS实测。

实际SDK合同新增分页拒绝行重试、raw受控修改/删除后SDK reclaim再继续原快照、新重启观察当前目录、dot模式/根父ID/无属性打包、非法cookie和被删除旧目录重启失败后仍可读旧verifier；256次新listing和80目录LRU压力保留，改为验证新token/不泄漏及显式STALE，64混合cursor下同目录重建也保留。标记 FSVOLUME_CAPTURED_DIRECTORY_CONTRACT_OK / FSVOLUME_FAILED_DIRECTORY_RESTART_CONTRACT_OK 必须读取准确macOS源码CI；不从Linux或类型检查预报成功。完整App/Extension、MSVC/Dispatcher、三平台core/native/旧reader拒绝与v1/v2/v3同容器artifact须逐项核验。

仍未完成：驱动/runtime及授权隔离环境的真挂载、macOS签名/系统授权、原生caller/ACL/共享/缓存通知/完整同步、符号链接/xattr/OS mmap/执行、真卷SQLite/Git/依赖/编译/IDE/Codex正常沙盒与越界拒绝、物理exFAT/USB/断电。安全入口继续关闭，#1/#3保持开放；没有操作用户本机、安装驱动或放宽沙盒。
