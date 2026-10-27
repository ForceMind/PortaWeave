# M0-T：有界目录元数据快照与 WinFsp ReadDirectory

2026-10-27 后续范围见 [M0-U](IMPLEMENTATION-M0U.zh-CN.md)：增加重新授权的当前 linked-directory GFI 与显式快照替换；本节下文保留 M0-T 当时范围。完整活目录 Open/删除后 inode、自动内核刷新/通知及原生门禁仍未完成。

日期：2026-10-26。基线 main `3198ac57df8a5b54762adc05868b68c0a0a411ee` / tree `d3ee3c6687b82a5ffa74ff24d1d0882dc8900ded`，重新读取后 14 项检查成功、无未合并 PR。继续已有 Dispatcher，不改格式、schema、依赖或工作流，不安装驱动。

## 核心与原生快照

`PosixSession::directory_snapshot` 在同一个独占 owner 借用内检查全部祖先 search 和选定目录 READ+SEARCH，并在 COUNT 预算预检后捕获目录、逻辑父目录、全部条目的原名及 FileInfo。子项的数据读取权限不影响可遍历目录中的 stat 元数据；v1 仍拒绝策略元数据，不伪造属性或自动升级。

快照是已经取得的只读名字/属性数据，不持有 inode、数据块或未来路径授权。后续 chmod、rename/unlink、目录移动/删除和 GC 不改旧副本；不能拿它读文件内容、重新获得名字或冒充活目录描述符。它不是 SQLite 外部写入/原始 backing 篡改下的快照协议；合法请求继续遵守单 owner 串行模型。

新增 C `pw_windows_directory_begin/read/close`。begin 完成可信 mode 策略、全部 Windows 时间/逻辑大小与 UTF-16 投影后才发布 token；失败不改输出或留下 cursor，不饱和转换。Windows 与旧管理 cursor 分表，但共用每 owner **64 个 cursor、100000 个保留条目**。非根的 `.`/`..` 也计入；root 不添加点项。现有 raw C 名字快照现在也受累计条目限制。逻辑条目数不是精确堆预算或 OOM 保护，仍缺完整解析器/GC 字节预算。

read 在单次原生 owner mutex 中取快照并完整编码输出，和 close 串行；没有跨条目查询/关闭窗口或后续属性 I/O。16 MiB 单次输出上限，允许零容量 NULL buffer；错误输出不变。活 cursor 阻止 volume close，释放只丢弃元数据快照，不执行 GC 或增加数据权限。token 不能与管理 cursor、文件 grant 或其他 volume 混用。

## 固定 WinFsp 2.1 格式

固定头与源码 `ddca7bd5481857a65ba552f643b8776fd070836f`。Rust 独立编码 FSP_FSCTL_DIR_INFO 的 104-byte header、UTF-16LE 非 NUL 名称、8-byte 对齐/全零 padding 和 2-byte 零 EOF。实际 MSVC translation unit 用 SDK 静态断言核对 header/字段 offset/WCHAR/对齐；没有复制 WinFsp DLL/helper 实现或新增加驱动/DLL依赖。

Dispatcher 和合同共用 volume 参数初始化，设置 `DirectoryMarkerAsNextOffset=1`。Marker 是指向 8 字节 cookie 的指针，不能读成字符串；每条 NextOffset 是下一 ordinal，同一 snapshot 的 offset=0 重放确定，offset=count 可读 EOF，越界拒绝。只写完整记录；缓冲不足以放下一条时成功返回零、不消费条目，若 EOF 放不下则下一次以相同最终 cookie 获取。支持非对齐 marker/output，避免直接解引用未对齐结构。原名/大小写/名称比较规则不变。

ReadDirectory 对 synthetic directory context 调用新 ABI；Pattern 按固定 FSD 合同由驱动筛选，用户态不假装已经跑过原生模式匹配。Close 释放正确类型的 snapshot，混合 file/directory context 拒绝。目录 Read/Write/EOF 不作为普通文件操作；目录 GetFileInfo/SetBasicInfo 仍未实现，不从旧快照伪造当前信息。HardLinks 字段继续按固定 SDK 要求为 0，接口缺口仍 BLOCKED。

**未完成的原生语义**：现有 snapshot 不是活目录 open grant；NULL Marker 重放同一快照，不表示重建当前目录/事件缓存。目录 Open、当前 GFI、刷新/通知、sharing/delete/ACL 和内核生命周期仍待真正实现。Open/security/Flush 保持关闭，不能把当前源码描述成可用目录卷或普通 Windows 文件操作已通过。

## FSKit 共享额度压力

FSKit 沿用旧管理名字快照及 32-entry LRU。若 native begin 因共用额度返回 Resource，SDK session 只逐个关闭自己的旧 LRU 后重试；外部 typed Windows snapshots、其他 caller 的 cursor 和活 file pin 不被驱逐。被驱逐 verifier 明确 ESTALE；若仍无额度则保留拒绝，不假成功。这里不把管理名字快照变成经过 OS 身份认证的目录权限。

新增实际 SDK 合同：1 个 session 快照 + 62 个 Windows 快照 + 1 个 raw 管理快照耗满 64；新枚举可通过回收自己的旧快照准入，旧 verifier 失效，外部 Windows 快照仍能读出两个 dots 和 EOF。准确源码 CI 日志标记 `FSVOLUME_CURSOR_PRESSURE_CONTRACT_OK`；实际运行成功后才记 PASS。

## 验证与限制

Work Linux 锁定依赖的 default/all-feature workspace（各 32 suites，含 Disk Studio）、clippy/fmt、真实 C11 caller/C++17 header、Python 9 项与本地四阶段程序通过。新增核心 4/3 项（all-feature/default）、ABI 8 项（累计 31）和 native 共享条目预算单元 1 项。覆盖 read/search/祖先、不可读子文件元数据、v1、原属性/名字/逻辑父、删除后无 inode pin、保护下观察、projection refusal、typed token/BUSY、UTF-16 surrogate/255 单位边界、完整分页/EOF、输出哨兵与 4 个读线程和 close 的竞争。共享条目预算单元仅在 test 构建降低 ceiling 至 3，使用真实卷和两类 cursor；不是 100000 个真实名字的性能/压力验收。

首次 clippy 指出测试 decoder 的两个新 lint（is_multiple_of/as_chunks），已按建议改写后通过，没有降低检查。Windows CI 需实际 MSVC 编译并按真实 SDK 结构解析 callback 输出，标记 `WINFSP_DIRECTORY_CONTRACT_OK`；macOS 完整 App/Extension、SDK 压力和 artifact 往返以对应 PR 的准确 source/tree 为准。核心目录交接标记 `DIRECTORY_SNAPSHOT_HANDOVER_OK` 不等于 OS dispatch。

真实挂载、原生 caller/SID/ACL/继承/共享/锁/通知/执行/mmap、缓存/完整同步/安全卸载、Codex 正常沙盒和 backing 越界拒绝、同一 exFAT/USB/断电继续 BLOCKED/NOT_RUN。未操作用户电脑，未启用扩展、安装驱动、放宽沙盒或操作物理介质。

## 原始依据

- 固定回调与布局：https://github.com/winfsp/winfsp/blob/ddca7bd5481857a65ba552f643b8776fd070836f/inc/winfsp/winfsp.h 及 inc/winfsp/fsctl.h
- 对齐/EOF helper 语义（阅读源码，未复制实现）：https://github.com/winfsp/winfsp/blob/ddca7bd5481857a65ba552f643b8776fd070836f/src/dll/fsop.c
- 64-bit marker 与 FSD 筛选：https://github.com/winfsp/winfsp/blob/ddca7bd5481857a65ba552f643b8776fd070836f/src/sys/dirctl.c 及 https://winfsp.dev/apiref/
