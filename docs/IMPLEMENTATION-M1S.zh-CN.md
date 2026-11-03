# M1-S：现有原生owner的空闲维护线程与退出join

2026-11-03，接续PR #47 main `d76bd485b9534d053152d28b773be05ee2899ffe` / tree `e33866359a8d947020b10ba8f16a99cb15421c18`。该main已独立14/14检查和5工作流成功，完整run/job/step审计PASS；随后才开展本增量。见ADR0020。

新增显式C管理入口 maintenance_worker_start/stop/status。worker只保存原session ID，不复制Volume、不另开SQLite/owner锁；用stop channel等待、try_lock既有registry后调用M1-R step。前台操作完成后重新计算quiet period，health/space/status轮询不把自己当成用户I/O。无进展/全pin/HostSpace延期退避；真正I/O StorageFull即便C码同为NO_SPACE，也不能按HostSpace重试，必须保留core保护并停止线程。

启动参数idle=10..600000ms、interval=10..60000ms；二次启动BUSY，已结束的线程也先stop释放；保护owner拒绝start。启动线程失败不发布半控制状态、不投毒核心。状态结构40 bytes，包含running、last_status、ticks和上次成功report；零或Healthy不表示无积压，失败也不承诺此前清理回滚。

stop发消息并join。worker绝不阻塞等待registry锁，因此可以在volume_close持锁时stop/join；有BUSY pin的close继续保留线程和owner，真正close才先join再drop。Session字段/Drop顺序同样确保线程先于Volume结束。stop等待已有core step结束后才能获得registry，不强杀线程或宣称硬卸载时延。卸载native库前须关闭全部owner。

WinFsp scratch lab在core创建后显式启用30秒空闲/1秒间隔，并在销毁平台状态前stop/join；runtime/driver缺失仍原样BLOCKED。普通core/C create/open没有暗中启动worker，FSKit资源/load门禁未开放也不启动线程。不是所有客户端已默认空闲回收，更不是原生卷已能挂载。

新增native单元逻辑测试区分单调quiet/backoff与真实StorageFull；新增integration通过公开Rust API产生129个持handle的已unlink文件，结束owner后C重开只清128，实际50ms/10ms worker在持续health/space/status轮询下自动清最后一项，再验证BUSY保持、stop/restart、成功close自动join及真正释放独占锁。该fixture不是safe-unmount或物理断电。C caller三格式实际start/status/stop/join并检查40-byte结构，输出 `NATIVE_MAINTENANCE_WORKER_LIFECYCLE_OK`；不等待默认30秒、不冒充实际驱动。

本地仍无Rust工具链。已运行Python19、严格C11/C++17语法和diff检查；新Rust、线程真实执行与WinFsp/SDK/App/跨OS结果必须以本增量准确source CI及独立main记录为准。所有有效原生身份/ACL/共享/完整同步、硬链接后端、真实挂载与卷内Codex/物理exFAT门禁保持，未操作用户本机/驱动/服务/沙盒。后续需完整平台owner授权/生命周期、强同步与资源预算，不能从userland worker通过推导这些验收已完成。


## 初轮静态检查与补充错误路径

初始source572caf74e5180fc2eb93d21c08ec495bd971f712 / treef0b8264a99f0e3f2d6cfebe48f7541ebceaa5322保留。Linux125761509120、macOS125761509360的功能/default及线程实际测试通过，但run41589129062在clippy::while_let_loop失败；后续fmt步骤因此skipped，不计通过。改成等价while-let等待循环，不降低-D warnings；相关日志/源码与evidence JSON保留，修正source须完整重新验证。

另补受控controller线程panic的unit：join失败经volume_close返回INTERNAL，原core owner仍注册且独占锁Busy，状态保留错误，后续显式close才释放。该panic刻意发生在registry/core之外，不冒充持锁core panic或恢复poisoned registry；后者仍拒绝服务。新增native unit合计2项，integration仍1项；无本地Rust通过声明。
