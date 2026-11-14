# M0-AC：Windows 跨进程共享与字节锁探针

日期：2026-11-14。扩展既有宿主探针；Windows 实际结果尚待对应源码 CI。

## 实现

既有 `windows-share` 和 `windows-lock` 用例现在启动一个独立 Python 子进程，
由父子进程分别通过真实 `CreateFileW` 打开同一私有夹具。两个方向都验证
共享/锁冲突和显式释放后的重试。使用不同 PID、卷序列号和 128 位 file ID
核对进程与文件身份；不以复制文件或继承句柄代替独立打开。

- 共享：父只读共享时子兼容读成功、写打开及删除精确拒绝为 Win32 32；
  父释放后同一子进程重开成功。另测新共享掩码必须容纳已有写访问，及子持有
  限制句柄时父写打开拒绝、子关闭后父重开成功。
- 字节锁：父独占 `[0,4)` 时子重叠锁/读/写精确拒绝为 Win32 33，相邻
  `[4,8)` 可锁；共享锁允许读、拒写与独占升级。再由子持锁，父验证拒绝，
  子显式释放后父重试，最后核对数据未改变且子进程已经退出。

固定 helper 为 `tools/windows_semantics_child.py`。它只接受 harness 创建的
私有目录及两个固定夹具名，保留 `.pwvol` 与实际/设计 raw layout 拒绝。
Python `-I -u` 和固定 sibling import 不接受 cwd/PYTHONPATH 选择代码；
checkout 本身仍可能位于被测卷。启动使用 `close_fds=True`，文件句柄由各
进程独立创建且不继承，不修改账户、ACL、服务、驱动或设备。

## 有界进程协议

每条消息最多 4096 字节、每会话最多 16 请求；默认单条 10 秒、会话 30 秒。
验证协议版本、序号、PID、操作、字段集合和完整文件身份；拒绝重复 JSON key、
错误/缺失/超长回复。父进程对管道读写设置期限，失败后终止并等待本次创建的
子进程、join 管道线程、关闭管道。成功还要求 quit 回复、零退出码和 EOF。

协议/超时/子进程故障直接进入 FAIL；不能只搜索成功文本，也不能将部分
步骤成功转成整个用例通过。异常退出后的 OS 锁回收延迟、mmap、不同安全
身份的 ACL 行为不在本轮范围内。

## 本地验证与远端待验收

Linux 本地全 Python 套件 29 项：28 通过，1 项真实 Win32 集成明确跳过。
测试实际创建子进程，覆盖错误 PID/序号、未知/重复字段、坏 JSON、超长行、
没有换行、提前退出、子错误、启动/回复/退出超时、非零退出、额外输出、
父异常、错误文件身份，以及失败后私有夹具清理和用户哨兵文件保留。
`py_compile` 和 `git diff --check` 通过。

原有 `.github/workflows/probe-baseline.yml` 已运行这两个用例，无需新增
工作流或权限。Windows 成功时需在对应源码日志看到
`WIN32_CROSS_PROCESS_SHARE_OK` 和 `WIN32_CROSS_PROCESS_LOCK_OK`，含父子 PID
与完整文件身份。此前正常 git push 曾两次被自动审批审查拒绝：审查认可
公开仓库归属及既有授权证据，但仍要求用户在本轮明确批准公开发布。
这两次拒绝保留为历史；当时没有改走连接器或间接途径发布。

用户现已针对推送本轮两项改动到 `ForceMind/PortaWeave`、创建 PR、通过检查后
合并的具体请求回复“好的 允许”。本轮已获明确授权，通过正常 GitHub 流程
提交并运行对应源码 CI。准确 source/main 的平台结果与最终合并状态以对应 PR
验收记录为准；没有相应源码成功证据时，Windows 应保留 NOT_RUN，不能把
Linux 协议测试计作 Windows 文件语义通过。

此增量可复用于将来的真实挂载后端，但当前不会关闭 FS-03/FS-08。
`mount_identity_verified=false` 和 `product_compatibility_certified=false`
继续保留；同账户宿主探针不认证随卷挂载、跨账户权限、映射锁或 Codex。

## 官方依据

- [CreateFileW 的双向共享访问规则](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
- [LockFileEx 的共享/独占与进程退出边界](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex)
- [GetFileInformationByHandleEx](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfileinformationbyhandleex)
