# M0-X：WinFsp Dispatcher 生命周期错误传播

日期2026-10-29，接续main4766e945。修复既有lab入口先启动dispatcher再注册控制信号、忽略wait失败以及未观察异常dispatcher停止的问题。此增量未安装驱动、未运行真实挂载，也不启用Open/security/Flush。

新增可移植顺序执行器：prepare→start→wait→stop→release；prepare/start失败不进入后续运行阶段，但仍释放部分prepare状态。start失败后的底层filesystem对象由既有destroy路径清理；成功start后，即使wait失败也调用stop。初始失败与release失败按bitmask同时保留，不能以清理成功吞掉等待错误。执行器只管理调用顺序，不证明WinFsp内部线程和缓存安全。

Windows在start前创建manual-reset事件并注册控制handler。仅Ctrl+C/Break走协作停止；关闭窗口、注销或关机不是安全卸载承诺。wait只接受WAIT_OBJECT_0，失败返回非零；服务独立设置DispatcherStopped回调，异常停止只记录原子标志并唤醒主线程，不在回调递归stop/delete。正常stop的回调不清除已记录异常；stop返回后再检查一次，以免Ctrl信号先唤醒、异常随后在stop阶段到达而误报正常。停止后解除handler并释放事件，失败保留非零结果。

事件发布/SetEvent/关闭由SRW锁保护，关闭等待正在发信号的共享读者退出。release只处理自己拥有的事件；第二个被拒绝的prepare不能关闭第一会话事件。原主线程继续按destroy→core close→DLL unload释放，stop成功不是fsync、safe eject或断电安全证据。

测试：Work Linux实际C11 -Wall/-Wextra/-Werror编译并运行可移植8种故障组合，验证准确顺序和初始+清理双错误；Python15项通过。Windows --self-test复用这些用例，并使用真实Win32事件/handler/SRW和fake dispatcher测试正常Break、wait前/stop中异常停止、第二owner拒绝/释放隔离及退休后信号拒绝。Windows行为必须由准确source的MSVC CI实际执行后才能算通过。CI没有driver安装和--lab-mount调用。其余三平台Rust/C/Swift/FSKit、完整App及跨OS交接仍需按准确源码核验。

主要依据：Microsoft [HandlerRoutine](https://learn.microsoft.com/en-us/windows/console/handlerroutine) 说明控制handler运行在线程中，close/logoff/shutdown可在返回后终止进程；WinFsp [DispatcherStopped API](https://github.com/winfsp/winfsp/blob/master/doc/WinFsp-API-winfsp.h.md) 区分正常和异常停止。代码仍按固定2.1头文件编译，不能把文档存在当挂载成功。

仍BLOCKED/NOT_RUN：真实驱动挂载、原生身份/ACL、共享/系统锁、完整同步/缓存、硬链接后端缺口、映射/执行/xattr、卷内Git/SQLite/Codex和物理exFAT/USB/断电。所有既有安全门禁保留，无本机操作、驱动安装、服务配置或沙盒降级。
