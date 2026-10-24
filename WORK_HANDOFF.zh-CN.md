# PortaWeave（随卷）持续研发接管

仓库 `ForceMind/PortaWeave`，公开，默认分支 main。先重新读取分支、PR、CI 和 Issue；不要假定本文里的 SHA 永远最新。用户禁止未经单独授权操作其本机、安装本机驱动、格式化介质或部署网站。

## 当前工程

已有 M1-A～M1-F Rust 持久化核心增量、统一原生 C ABI、Windows WinFsp Dispatcher 实验源码，以及 macOS Xcode 宿主 App/ExtensionKit 工程。不是可用的系统文件卷。

- PR #13（M1-E）已合并，主线合并 SHA `b10ccd5ca09c374b8cab6b77dbbe9c613ba6df92`：范围租约只索引区间、保存原 EOF/旧版本、预算共用、C ABI token/关闭 BUSY。
- PR #14（M0-J 第一增量）已合并，主线合并 SHA `c6691499055586b781c93ed789f82683ffdc8fb0`：真正 App+Extension 构建和静态链接，准确源码 CI 为 `7204052b55b46c6b943445233154533e10d20264`，Xcode 26.6 / SDK 26.5。没有开发者签名、公证或启用挂载。probe/load/unload 仍 ENOTSUP，FSVolume.Operations 尚未实现。
- PR #15（M1-F）拒绝未知 catalog schema；普通打开在 config/WAL配置/GC 前验证，恢复只检查工作副本。只规范 SQL CRLF→LF，不能绕过 Windows 校验。最终准确 head/测试/合并 SHA 以 PR #15 和实时 main 为准。
- Windows CI 新增只读运行时/驱动清单；以原始 JSON 记录是否具备已安装 WinFsp，不安装、不启动/配置服务、不创建盘符。

阅读 AGENTS、SPEC、COMPATIBILITY、所有 IMPLEMENTATION-M0*/M1*、VALIDATION 与 ADR。保留早期失败及修复证据：格式失败、FSPathURLResource target 失败、otool header 误判，以及 schema LF/CRLF 交接拒绝。不能盲目重跑掩盖差异。

## 不可降低的目标

exFAT 上同一动态容器，两端真正挂载成开发工作盘。Codex 正常沙盒、Git/worktree、IDE、包安装、编译执行、监听、锁、mmap、链接和有效权限必须在真实卷里验证。不得复制工作树到系统盘，不得关闭沙盒、放宽全局权限、伪造硬链接/flush。单 owner，安全卸载后顺序交接。

Recovery 保全原始文件并在独立副本恢复 SQLite/WAL；普通 Volume::open 仍是可写恢复入口。内容导出不等于完整元数据备份，未知 schema 拒绝不等于修复。成功业务和后续维护失败分离，实际 commit 错误仍可能结果不明。

## 下一阶段

1. 实现 FSKit FSVolume 生命周期、目录/文件操作和 security-scoped resource 的持有/释放；先补必要的时间/权限/执行位/链接/xattr 与真实同步。签名及系统授权需合适的已授权环境，不把类型检查或打包当真挂载。
2. 推进 WinFsp ACL/共享/删除/缓存/同步及缺失回调；硬链接接口缺口仍 BLOCKED，需要上游或替代后端方案，不能用核心硬链接关闭。
3. 继续核心空间预约、应急预留、SQLite VFS 故障矩阵、有界空闲维护、UUID/能力位、名称版本、迁移与完整解析器资源预算。
4. 真挂载后完成 Git/npm/pnpm/Python/Cargo/IDE/SQLite 与实际 Codex 正常沙盒的正向和越界拒绝测试，原始 backing 目录也纳入权限边界。
5. 再做真实 exFAT 介质往返、授权故障盘实验、安装签名/公证和稳定性发布。

CI 容器传递不是同一硬盘插拔，进程退出不是物理断电。没有环境/驱动/签名/授权的项目保留 BLOCKED/NOT_RUN。每次实际测试、提交、合并并核验远端 SHA；不承诺会话结束后的无人值守研发。
