# M0-Z：Windows 原生共享、锁及打开文件生命周期探针

日期：2026-10-30。接续已合并 PR #43 的 main `8da34020c5b9ffde2abf4f0730d1470c0cdb85b5` / tree `6c9d68acf289fe45e9b504653e02f056e7a0db05`。M0-Y 准确源码 17/17 检查、6 工作流完整通过；其主线检查单独追踪，不以源分支结果冒充。

既有 workspace probe 的 Windows replace 只在文件关闭后调用 Python replace，exclusive 项只验证 O_EXCL，不能验证 Windows 共享模式或字节锁。新增 `tools/windows_semantics.py`，通过明确 argtypes/restype 的 ctypes 调用真正 Win32 API，不构造 FSP mock、不调用核心协作锁。继续使用统一 JSON 报告、排他写报告、私有临时夹具和 read-only CI 权限。

| 可选择 case | 实际检查 |
|---|---|
| windows-share | 兼容 READ 打开；独立句柄身份相同；旧 share 拒绝新 WRITE/delete；新 share 拒绝旧 WRITE 权限；关闭后新 WRITE 打开恢复 |
| windows-lock | 两个独立同步句柄执行 LockFileEx；exclusive 冲突、第二句柄 ReadFile/WriteFile 被锁拒绝、owner 可读；相邻范围允许；重叠 shared 允许读但 owner 写也被拒绝；第三句柄 exclusive 被拒绝；unlock 后重新取得锁 |
| windows-delete | 明确 FileDispositionInfo 旧式 delete-pending，新打开必须 ACCESS_DENIED，原句柄可读且最后 close 后 FILE_NOT_FOUND；另测 DeleteFileW 并记录其即时 unlink 或 pending 可见性，始终验证旧句柄身份/内容和最终消失 |
| windows-replace | 源/目标均以 share-delete 保持打开；先 MoveFileExW 将源改为不存在的名字，再通过有 DELETE 权限的源句柄执行 FileRenameInfoEx(REPLACE_IF_EXISTS|POSIX_SEMANTICS) 替换打开的目标；新名字保持源 ID/内容、旧目标句柄保持旧 ID/内容，旧源及中间名字消失，关闭重开保留新内容 |

共享冲突要求 Win32 32，锁冲突要求 33，不把任意失败当预期拒绝。LockFileEx 一律 FAIL_IMMEDIATELY，不允许本线程等待自己必须释放的锁；没有 OVERLAPPED 异步 I/O。同步句柄不可继承，所有 admitted locks 在 finally 显式 unlock，所有句柄 CloseHandle；意外成功的负向 open/lock 也先释放再报告失败。文件一律 xb 创建，只使用 runner 生成的私有目录；不覆盖现有报告或用户文件。基线 CI 有 10 分钟 job 上限。

增强统一入口安全检查：除 .pwvol 路径外，任一祖先同时存在 format.json 和 catalog.sqlite 时，在新建临时目录前拒绝。这样重命名容器去掉后缀也不会被当作工作盘根。只检查布局是否存在，不打开/恢复原 catalog、不读取 marker 内容；这是防误用，不是对恶意目录竞态/设备路径的安全边界。公开探针仍须由统一 workspace_probe 入口调用，不直接把模块函数当未授权路径接口。

使用示例（只可传新建的可丢弃工作目录，不能传 backing 容器）：

```sh
python tools/workspace_probe.py --root /path/to/disposable-directory --cases windows-share,windows-lock,windows-delete,windows-replace --label unverified-target --report new-report.json
```

在 Windows 云 CI 的 RUNNER_TEMP 首先验证宿主 API/测试器。JSON 的 mount_identity_verified/product_compatibility_certified 始终 false；标签不是挂载身份证明。Linux/macOS 四项 NOT_APPLICABLE，不计为四项 PASS；FAIL/NOT_RUN 仍使 CLI 非零退出。现有其余 8 项 host 基线保留，所有三平台 core/native/App/交接 CI 不缩减。

本轮 Work Linux：Python 17 项实际通过（原 15 + raw-layout 原件/子目录不修改拒绝 + 非 Windows 不误报通过），py_compile 和 diff check 通过；四个 Windows case 实际报告 NOT_APPLICABLE。没有本地 Windows 成功声明。准确源码的 Windows host 日志/JSON 中四项 PASS 后才记宿主验证通过；完整源/主线 CI 和 SHA 记录在对应 PR。

这些是一个进程的多个独立真实 Win32 句柄，不是跨进程、不同 token/ACL/继承、OS sandbox 或 mmap 锁测试；微软明确映射视图不受这些 byte-range locks 的同样约束。本测试不验证 FlushFileBuffers、fsync、safe-eject、硬件持久化、真实挂载身份或任何 native adapter 实现。未来真卷须另行记录挂载与平台/驱动身份并重新运行；不能用宿主 PASS 给 WinFsp 共享/系统锁打勾。WinFsp 硬链接后端缺口、原生授权与所有挂载/开发工具/Codex/物理 exFAT 门禁不变，仍 BLOCKED/NOT_RUN。

首轮失败记录保留：source `54b9bd1ab60f0232a2fdc8a463ee1b8038b74754` / tree `93b9f3e7e8134bb3c0b0a48d45d89a6650432c08` 的 PR host run41033742005 / Windows job123989413453（另 push host41033734714）为 10 PASS/2 FAIL：旧测试把 DeleteFileW 的新打开错误固定为 5，实际为 2；MoveFileExW 替换打开目标报告 Win32 5。共享和真正锁执行检查已通过，没有改写失败结果或重跑相同源码来隐藏问题。

核对 Microsoft WIL 源码后，DeleteFile/RemoveDirectory 现在先尝试 POSIX delete，失败才回退；不能仅依据简略 API 页面就把旧式 pending 当作唯一可见性。修正为显式测试旧式 FileDispositionInfo（BOOLEAN 一字节）和独立观测 DeleteFileW（只允许 2/5 且报告实际值，其他错误仍 FAIL）。打开目标替换改用正式 FileRenameInfoEx POSIX 合同，不把它等同于旧 MoveFileExW。新增 Win32 类型/结构大小与 x86/x64 alignment 运行时断言，所有布局在准确 Windows CI 实测，不用 Linux ctypes 的宿主布局替代。后续修正 source 需重新跑完整 CI；旧失败留存。

主要依据：Microsoft [Create/open sharing matrix](https://learn.microsoft.com/en-us/windows/win32/fileio/creating-and-opening-files)、[LockFileEx](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex)、[DeleteFile](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-deletefile)、[MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)。另见 [Microsoft WIL delete 实现](https://github.com/microsoft/wil/blob/master/include/wil/filesystem.h)、[FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)、[FILE_DISPOSITION_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_disposition_info)。不使用 DELAY_UNTIL_REBOOT/COPY_ALLOWED，不操作注册表、驱动、ACL、用户电脑、真实重要文件或物理磁盘。
