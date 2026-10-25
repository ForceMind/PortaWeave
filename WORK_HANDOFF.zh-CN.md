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
- M1-K 增加不透明 PosixFile 数据能力与当前 metadata 修改授权，token 与管理表分离、共同 4096 限额/BUSY，旧 FD 权限保持至关闭。混合变更同时检查 WRITE 和当前 owner/group，仍为一次核心提交。准确 CI/head/合并状态见对应 PR。

阅读 AGENTS、SPEC、COMPATIBILITY、所有 IMPLEMENTATION-M0*/M1*、VALIDATION 与 ADR。保留早期失败及修复证据：格式失败、FSPathURLResource target 失败、otool header 误判，以及 schema LF/CRLF 交接拒绝。不能盲目重跑掩盖差异。

## 不可降低的目标

exFAT 上同一动态容器，两端真正挂载成开发工作盘。Codex 正常沙盒、Git/worktree、IDE、包安装、编译执行、监听、锁、mmap、链接和有效权限必须在真实卷里验证。不得复制工作树到系统盘，不得关闭沙盒、放宽全局权限、伪造硬链接/flush。单 owner，安全卸载后顺序交接。

Recovery 保全原始文件并在独立副本恢复 SQLite/WAL；普通 Volume::open 仍是可写恢复入口。内容导出不等于完整元数据备份，未知 schema 拒绝不等于修复。成功业务和后续维护失败分离，实际 commit 错误仍可能结果不明。

## 下一阶段

1. 继续范围租约授权及原生 caller/内核权限委托，再衔接 FSKit 策略、security-scoped resource、符号链接/xattr 和真实缓存/同步。受信任目录/数据/属性策略已有，但原生 callback 未绑定可信身份，不能开放；创建 size/time 仍未支持。签名及系统授权需合适的已授权环境，不把直接 SDK 调用或打包当真挂载。
2. 推进 WinFsp ACL/共享/删除/缓存/同步及缺失回调；硬链接接口缺口仍 BLOCKED，需要上游或替代后端方案，不能用核心硬链接关闭。
3. 继续核心空间预约、应急预留、SQLite VFS 故障矩阵、有界空闲维护、名称版本、明确迁移工具与完整解析器资源预算；不得静默把 v1 升成 v2。
4. 真挂载后完成 Git/npm/pnpm/Python/Cargo/IDE/SQLite 与实际 Codex 正常沙盒的正向和越界拒绝测试，原始 backing 目录也纳入权限边界。
5. 再做真实 exFAT 介质往返、授权故障盘实验、安装签名/公证和稳定性发布。

CI 容器传递不是同一硬盘插拔，进程退出不是物理断电。没有环境/驱动/签名/授权的项目保留 BLOCKED/NOT_RUN。每次实际测试、提交、合并并核验远端 SHA；不承诺会话结束后的无人值守研发。
