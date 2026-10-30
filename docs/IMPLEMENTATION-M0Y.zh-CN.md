# M0-Y：原生适配层错误分类与冲突恢复

日期：2026-10-30。接续 main `a9aff93fb09c57e917e8aa60bbe709cb10545df1` / tree `4bcb5eee064a739fa14e28aec1b2dd72977d9aab`。重新读取 GitHub 后，M0-X 原排队三项已完成，主线 14/14 检查成功，无开放 PR；#1/#3/#28 保持开放。

既有 Windows `PwNtStatus` 默认把新增及未知状态都转换为 INVALID_PARAMETER，导致内部故障、协作锁竞争、过时名字观察无法区分。现在明确映射：

| 核心结果 | Windows 适配结果 | FSKit pwCheck 结果 |
|---|---|---|
| PW_LOCK_CONFLICT | STATUS_FILE_LOCK_CONFLICT（新增） | EAGAIN（新增） |
| PW_STALE | STATUS_RETRY（新增） | ESTALE（既有） |
| PW_DENIED | STATUS_ACCESS_DENIED（保持） | EACCES（保持） |
| PW_INVALID | STATUS_INVALID_PARAMETER（显式） | EINVAL（保持） |
| PW_INTERNAL / 未知正负结果 | STATUS_INTERNAL_ERROR | EIO（保持） |
| PW_END 误入普通操作适配 | STATUS_INTERNAL_ERROR | EIO（保持） |
| PW_BAD_COOKIE | STATUS_INVALID_PARAMETER（显式，非 Windows cursor API） | 固定 SDK invalidDirectoryCookie（保持） |

PW_END 是专用 cursor 迭代协议的正常终止标记，须由对应迭代代码处理；通用操作检查器不能将其伪报成功。Windows PW_IO/DATA_ERROR、权限、配额和资源等既有分类不变，未知状态不能再被归咎为调用者参数错误。

STATUS_RETRY 是本适配层为受事务身份检查保护的 stale observation 选定的错误；必须重新观察名字/身份，再由调用者决定是否重发。**不自动重试删除或改名，不承诺 Windows 内核自动刷新或重试，也不表示整个卷被外部修改。** EAGAIN 同样是有限非阻塞协作 grant 的适配约定，不声称实现 macOS fcntl/flock 的进程所有权、范围替换及 errno 合同。WinFsp 当前没有系统锁回调或可用 Open，这一转换也不是 LockFileEx 已工作。

Windows MSVC 合同与 macOS Swift/FSKit 合同新增实际 C ABI 链路：打开同一 inode 的两个硬链接名字，持有写端 exclusive grant 后，另一 READ grant 申请 shared 产生真实 PW_LOCK_CONFLICT，经生产错误转换分别返回 FILE_LOCK_CONFLICT/EAGAIN；READ grant 申请 exclusive 则为拒绝访问。失败输出仍为 99；释放冲突后同一请求成功取得新的 token。用错误 expected ID 执行 checked remove 返回 RETRY/ESTALE，实际 alias 保留；对象世代、链接、大小、元数据与健康/写保护保持不变。关闭所有 grant/owner，保留既有完整合同。另检验内部/I/O/损坏/未知正负值和 END 的协议拒绝，不注入伪造的媒体故障来声称故障恢复通过。

新增日志：`WINFSP_ERROR_PROPAGATION_CONTRACT_OK` 和 `FSVOLUME_ERROR_PROPAGATION_CONTRACT_OK`。它们使用真实核心操作产生冲突和 stale，但只直接调用适配器；没有系统锁分发、实际挂载或 OS caller 认证。

Work Linux 本轮实际运行 Python 15 项与 C11 严格编译的既有 8 种 lifecycle 故障组合、diff check，均通过。Work 当前无 Rust/Swift/MSVC 工具链，新原生测试及完整 Rust 回归必须以本增量准确 source 的 CI 日志验收，不能把旧 main 的 CI 作为新代码通过。对应 PR 保留 source/tree、平台日志及合并 main 的检查记录；本文不预报排队结果。

未改格式/schema/依赖、授权规则、锁所有权或原生安全入口。Windows Open/security/Flush 与 FSKit access/probe/load/activate/mount 保持关闭；真实权限/ACL/共享/锁/缓存/同步、符号链接/xattr/映射/执行、卷内开发工具/Codex 正常沙盒与越界限制、真实 exFAT/USB/断电仍 BLOCKED/NOT_RUN。没有用户本机操作、驱动安装或沙盒降级。

依据：Microsoft [NTSTATUS Values](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-erref/596a1078-e883-4972-9bbc-49e60bebca55)；Apple [EAGAIN](https://developer.apple.com/documentation/foundation/posixerror/eagain)。错误选择是本工程适配决策，文档定义并不证明真实卷兼容。
