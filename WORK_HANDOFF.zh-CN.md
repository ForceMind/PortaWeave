# PortaWeave（随卷）持续研发接管

仓库 `ForceMind/PortaWeave`，公开，默认分支 main。先重新读取分支、PR、CI 和 Issue；不要假定本文里的 SHA 永远最新。用户禁止未经单独授权操作其本机、安装本机驱动、格式化介质或部署网站。

## 当前工程

已有 M1-A～M1-G Rust 持久化核心增量、统一原生 C ABI、Windows WinFsp Dispatcher 实验源码，以及 macOS Xcode 宿主 App/ExtensionKit 和 M0-K FSVolume 回调。M1-H 增加显式 v2 卷身份与 POSIX 元数据存储。不是可用的系统文件卷。

- PR #13（M1-E）已合并，主线合并 SHA `b10ccd5ca09c374b8cab6b77dbbe9c613ba6df92`：范围租约只索引区间、保存原 EOF/旧版本、预算共用、C ABI token/关闭 BUSY。
- PR #14（M0-J 第一增量）已合并，主线合并 SHA `c6691499055586b781c93ed789f82683ffdc8fb0`：真正 App+Extension 构建和静态链接，准确源码 CI 为 `7204052b55b46c6b943445233154533e10d20264`，Xcode 26.6 / SDK 26.5。没有开发者签名、公证或启用挂载。此阶段尚无 FSVolume.Operations，随后由 PR #17 添加；probe/load 仍关闭。
- PR #15（M1-F）拒绝未知 catalog schema；普通打开在 config/WAL配置/GC 前验证，恢复只检查工作副本。只规范 SQL CRLF→LF，不能绕过 Windows 校验。最终准确 head/测试/合并 SHA 以 PR #15 和实时 main 为准。
- Windows CI 新增只读运行时/驱动清单；以原始 JSON 记录是否具备已安装 WinFsp，不安装、不启动/配置服务、不创建盘符。
- PR #16（M1-G）已合并 `8cbcfc25e3d80f640863f1cecbdb9fb86a40b5c2`，Unix 后端存储屏障、busy/错误保护和非 Unix 拒绝；不等于安全弹出或内核缓存排空。
- PR #17（M0-K）已合并 `372c3f5a57c6078a68a0ba4d0edbb6290099d561`，准确 head `557a57e9e464c3f1ddf72889f88e56e6e1a88400` 的 17 项检查全部通过，源码/候选/合并 tree `cf3b0111bd499600be4b386faa0973b7c94524c7` 一致。实际 SDK 对象回调、C readback/枚举算法与 App/Extension 构建均有证据，但未安装/挂载。评审发现重复 listing 耗尽 cursor，修复复用和 32-entry LRU 后跑 256/80 压力测试。保留首轮 Swift Int32/Int 失败。
- PR #18（M1-H）已合并 `e8700c9d8e63bec331f396be6a2faa950f51e8c8`，准确 head `986c8c097f11fe05fbf68791537f38df90f00613` / tree `d96c3d018ba8760b61160a83a7ad2afb535700e5` 17 检查通过，v1/v2 实际 macOS/Windows/返回交接与恢复保全通过。v2 只能显式新建，默认 v1 不变、无自动升级。uid/gid/mode/时间/UUID 是存储基础，不是有效权限。
- PR #19（M0-L）已合并 `3098c4cb24a06d8a4695b82dc57ee444297e63a1`，准确 head `0ab866f517b417cb8b908e83ea5d2d0d79bfc012` / tree `4e6c1605c909c9f78acc847c2f5db70241707e4f` 17 检查通过。v2 UUID/元数据接入 FSVolume 身份、getAttributes 与枚举；实际 SDK v1/v2 合同通过。保留独立 session tag，v1 不伪造字段，设置与 access/load/mount 仍关闭。
- PR #20（M1-I）已合并 `1e0fc95fb28d099a1a640b3ecca3d1408e88c6a8`，准确 head `a32096f24ddd9db176f1684859e1d7a5e0c8103f` / tree `801c040038657946e8d4d342e26ba2f467ad0e8f` 17 检查通过。`truncate_with_metadata` / `pw_file_truncate_with_metadata` 单事务更新大小/块引用与元数据；显式 mtime 优先，截断最终清除 file setid，一次 generation。core 13/10 项套件、C caller、真实三平台 atomic artifact 交接和恢复通过。
- PR #21（M0-M）已合并 `3a5cd787519ecfba468addc5c96f9b776343c8cc`，准确 head `75a1a19d0e977d16028f216f3589ca0f8f403d5e` / tree `bb8ac4acbdd299bdd466885a0d8aaf4379bb2e9e` 17 检查通过，合并后 main 14 项也通过。FSKit v2 混合 setattr 和 uid/gid/mode 创建在实际 SDK 合同运行；access/load/mount 继续关闭。
- PR #22（M1-J）已合并 `7ac870cd2dd98d0d2f42783a32c99addfca700ff`，准确 head `65251b0414aa71c7b0d0e8c656bc8e881504a8c7` / tree `96efea5d77f7bb4812a108fbc7ec7da66b1c63be` 17 项检查通过。`PosixSession` 实施受信任 caller 的选定目录权限、全部祖先搜索、sticky 两端替换、umask/setgid 继承，独立 PW_DENIED 映射；合成凭据不等于 OS 身份认证。
- PR #23（M1-K）已合并 `3b89b92783bb1e7c94d2662a84071ee47e437e2f`，准确 head `0077e1660727391416b47830348830acf3ea934e` / tree `71c01f76b9e593a184179d2878173957b17c7a53` 17 项检查通过。不透明 PosixFile 数据能力、当前属性授权、两表共同 4096 限额/BUSY、旧 FD 权限及混合事务均有回归，真实交接含 descriptor 检查。
- PR #24（M1-L）已合并 `8334bc538396743edf16e204e18c66d64a54b8de`，准确 head `d458f8f0123e517356f1dc1c513a79cbef703261` / tree `b57feef2397d92a9818b3add59532440ea2ef962` 17 项检查通过，主线 14 项也通过。为 READ grant 接入不可变范围租约，策略/管理 view 分表，保留原范围/EOF/字节并共享准入，源 FD 关闭后仍 BUSY 至释放。进程内能力不会序列化跨系统传递；每阶段重新授权测试。准确 CI/head/合并状态见对应 PR。

PR #25（M1-M）已合并 `fe63c2db52308af77cd0ab9602f86f1f82da697b`，准确 head `3ad88cec27e544f003445a4b2de91b2d1c506eaa` / tree `0cae02573dd7427129254e58eeda9b7b720187ec` 17 检查通过。增加单请求追加/EOF 裁剪及事务内信息回执；同一核心写入/截断实现复用，策略 C API 的并发追加/拒绝输出有回归。准确源码 CI/合并状态以对应 PR 为准；WinFsp 回调、身份/ACL/共享/缓存/Flush 和实际挂载仍待接入验收。

PR #26（M0-N）已合并 `832023dab77429432b7035a17ab66fec7468fd50`，准确 head `513170fef5bbc16de8d0f1047fdb13bf580a465f` / tree `ae0e59696a8ad47f013fd6d3e6e450f2f638e0fb` 17 检查通过，主线 14 项也通过；真实 MSVC job 121154954006 输出 WINFSP_GRANTED_IO_CONTRACT_OK。衔接策略 grant 的 WinFsp GetFileInfo/Write/SetFileSize EOF、Read EOF/Close 状态和提交前 Windows 投影；allocation 请求拒绝，HardLinks 仍按固定 SDK 的未实现约束置零并 BLOCKED。准确 MSVC/回调执行与 source CI 见对应 PR；安全 Open/Flush 不开放。

PR #27（M0-O）已合并 `98675503ba6e80a765b697842f546eb459ace12d`，准确 head `3c54f674a3bfba2c11932aa6c45e8d3637bbd2f4` / tree `72f3e1cbe65751666b80ac3b4e964bd3b58097d3` 17 检查通过，主线 14 项也通过。修复 FSKit setattr 提交后再次查询回执的错误路径；通过事务内 ObjectInfo/纯投影验证 ID/数量/时间，成功后不再查询或 throws 转换。v1 size 明确没有元数据，管理/策略 token 分离，expected object 错配在提交前回滚；准确 SDK/source CI 见对应 PR，access/load/mount 不开放。

最新接管基线已推进至 PR #29 合并 `3c215486c3660259b0ee18b2b69ad678f50b4ee4`，14 项主线检查通过；Disk Studio 只读检查器与 ADR 0010 保持独立范围。PR #30（M0-P）已合并 `a3df83046093b505097d0cb02954c2add3fbded7`，准确 head `1c123ced61b17a294f01ee91a44aa0fa40e77ae8` / tree `03d322e531c1ca3278b833eb742faed19d6ba81b` 17 检查通过、主线 14 项也通过；SDK job 121176312211 实际通过 4096 准入合同。继续原文件卷主线，修复 create 成功后再 intern/open 的错误路径，新增原子创建+管理 pin/回执、SDK 预分配和共享句柄准入，准确 CI/合并状态见对应 PR。

M0-Q 衔接同事务初始逻辑大小（稀疏零读）和 v2 birth/access/modify time；v1 仅 size，目录 size 明确不支持。generation=0、配额/格式/时间拒绝、提交前后故障和 pin 保持均有回归；准确 source/SDK/build/交接 CI 见对应 PR。不改 schema、不开放 OS 权限/load/mount。

M1-N / M0-R（2026-10-26）继续当前主线：活策略文件的当前属性授权与提交前回执，C 通用 metadata-info API，以及固定 WinFsp SetBasicInfo 创建/访问/修改时间合同。只读数据 grant 不妨碍当前 owner 修改属性，旧 WRITE 不绕过 chown；DOS flags/显式 ctime/控制时间值在修改前拒绝。测试 metadata_caller 必须显式且 NULL 拒绝，不是 SID/ACL/WRITE_ATTRIBUTES；原生安全/Open/Flush 继续关闭。准确 source/tree/三平台 CI、MSVC 与 artifact 返回以对应 PR 为准。

M0-S（2026-10-26）接续删除/改名的同请求、事务内 expected source/target 校验；FSKit 不再用独立 lookup 保护随后按名字修改。PW_STALE 映射 ESTALE，大小写同目录项与硬链接 alias 分别处理。真实 C 线程受控交错、提交前后故障、SDK v1/v2 与 artifact 标记见对应 PR；不是 OS 身份/缓存/挂载或完整 ABA 世代。

2026-10-26 句柄准入补正：原始 Rust open_file 现复用统一 reserve/publish；修复可绕过 4096 总额度的管理入口。现有 admin-only 与混合 policy/admin 测试强化，保留修复前失败证据；逻辑预算不是精确堆/OOM 保护。详见 M0-P/ADR0011 和对应 PR。

M0-T（2026-10-26）增加权限检查后的不可变目录元数据、typed Windows snapshot 与 ReadDirectory 固定格式、UTF-16/cookie/EOF；Windows/admin 共用 64 cursor/100000 条目，FSKit 只回收自己的旧 LRU。不是活目录 grant/current GFI/刷新，原生 Open/security/Flush/挂载继续关闭；准确 source/MSVC/SDK/bundle/artifact 见对应 PR 与 ADR0013。

阅读 AGENTS、SPEC、COMPATIBILITY、所有 IMPLEMENTATION-M0*/M1*、VALIDATION 与 ADR。保留早期失败及修复证据：格式失败、FSPathURLResource target 失败、otool header 误判，以及 schema LF/CRLF 交接拒绝。不能盲目重跑掩盖差异。

## 不可降低的目标

exFAT 上同一动态容器，两端真正挂载成开发工作盘。Codex 正常沙盒、Git/worktree、IDE、包安装、编译执行、监听、锁、mmap、链接和有效权限必须在真实卷里验证。不得复制工作树到系统盘，不得关闭沙盒、放宽全局权限、伪造硬链接/flush。单 owner，安全卸载后顺序交接。

Recovery 保全原始文件并在独立副本恢复 SQLite/WAL；普通 Volume::open 仍是可写恢复入口。内容导出不等于完整元数据备份，未知 schema 拒绝不等于修复。成功业务和后续维护失败分离，实际 commit 错误仍可能结果不明。

## 下一阶段

1. 继续原生 caller/内核权限委托，再衔接 FSKit 策略、security-scoped resource、符号链接/xattr 和真实缓存/同步。受信任目录/数据/属性/租约策略已有，但原生 callback 未绑定可信身份，不能开放；创建时普通文件 size 与 v2 初始时间已接入实验合同；仍缺原生 caller/完整文件元数据。签名及系统授权需合适的已授权环境，不把直接 SDK 调用或打包当真挂载。
2. 推进 WinFsp ACL/共享/删除/缓存/同步及缺失回调；硬链接接口缺口仍 BLOCKED，需要上游或替代后端方案，不能用核心硬链接关闭。
3. 继续核心空间预约、应急预留、SQLite VFS 故障矩阵、有界空闲维护、名称版本、明确迁移工具与完整解析器资源预算；不得静默把 v1 升成 v2。
4. 真挂载后完成 Git/npm/pnpm/Python/Cargo/IDE/SQLite 与实际 Codex 正常沙盒的正向和越界拒绝测试，原始 backing 目录也纳入权限边界。
5. 再做真实 exFAT 介质往返、授权故障盘实验、安装签名/公证和稳定性发布。

CI 容器传递不是同一硬盘插拔，进程退出不是物理断电。没有环境/驱动/签名/授权的项目保留 BLOCKED/NOT_RUN。每次实际测试、提交、合并并核验远端 SHA；不承诺会话结束后的无人值守研发。
