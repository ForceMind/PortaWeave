# M0-N：WinFsp 授权普通文件的真实回调合同

继续已有 Dispatcher，在固定 WinFsp 2.1 API 上实现 `GetFileInfo`、`Write`、`SetFileSize`（仅 EOF），并把 `Read`/`Close` 从管理 token 改为策略文件 grant。CI 仍直接构造 FSP_FILE_SYSTEM/context 调用这些真实签名，不安装驱动、不创建盘符。`Open`/`GetSecurityByName` 继续 ACCESS_DENIED，`Flush` 继续 NOT_SUPPORTED；Create/ReadDirectory/ACL/rename 等仍未接入。

## 数据与属性

通过 `pw_windows_file_info/write/set_size` 使用 v2 策略文件；合成 fixture 的授权不是 OS 身份认证。只读 grant 不能写入/改大小，管理/失效/其他 owner 的 token 不可混用。旧打开 inode 在最终 unlink 后仍能读写和查询身份；Close 成功清 token，保留关闭状态，不强制释放活动 owner。

Write 支持普通偏移、追加和 constrained I/O；位置与当前 EOF 在同一个核心请求中解析，直接返回事务内完成的信息。Read 在非零请求到达 EOF 时报告 END_OF_FILE，零长读取仍成功；截断再增长读取零。SetFileSize 的 allocation 请求明确未支持，拒绝前不截断、不伪造预留；逻辑 EOF 与物理分配不是一回事。

Windows 投影在原生 bridge 中独立完成，不更改核心格式：时间按 FILETIME 100 ns 向下量化，原始纳秒不改写；选择 `[0, INT64_MAX]` 的可表示范围，越界明确 unsupported，不饱和夹成另一时间。逻辑 AllocationSize 使用 Dispatcher 的 4096-byte 几何向上取整且不超过 i64::MAX，**不是宿主实际占用或预分配承诺**。FILE_ATTRIBUTE_NORMAL 不由 Unix mode 推导 ACL/readonly；没有持久 Windows DOS flags。固定 WinFsp 头的 HardLinks 字段仍要求填 0，其接口缺口继续 BLOCKED，不能当成硬链接功能已实现。

纯、可失败的 Rust reply projection 在业务事务 commit 前运行；转换失败回滚 size/blocks/metadata/generation，可能留下既有可回收 staging 段，不丢旧数据。C 回调成功路径只做结构复制与有界数量转换，提交后没有第二次属性查询或可失败时间转换。不得把可执行/外部有副作用的回调塞进这种 projection。详见 ADR 0009。

## 验证层级

Work Linux 实际 full default/all-feature workspace、clippy/fmt、真实 C caller/C++ header 通过；新增核心 reply-rejection 1 项（write_requests 累计 8/6）、Windows FILETIME 单元 1 项及 ABI 2 项（累计 11）。覆盖 epoch 前后/100 ns 边界/上下限、错误输出、拒绝后全属性及内容不变、修复属性后继续写、读授权/管理/关闭 token。

Windows CI 的 MSVC `/WX` 将实际编译并执行回调 fixture，断言普通读写、追加、裁剪、零写、文件身份、时间/逻辑 allocation、只读拒绝、非法输出、allocation 拒绝、截断/增长读零、EOF、旧 inode、转换失败回滚、关闭失效及安全 Open/Flush 不开放；实际通过以对应 PR 准确 SHA 日志中的 WINFSP_GRANTED_IO_CONTRACT_OK 为准，Work Linux 不预报 Windows 回调结果。Dispatcher `--self-test` 同时验证新增函数表和仍然关闭的入口；手动 `--lab-mount` 的既有新建 v1 scratch 路径不因此开放任何文件。

真实 Windows token/ACL/继承、共享访问、缓存、Flush、驱动挂载、执行/映射/通知，以及 macOS 真实挂载、Codex 正常沙盒/越界负向、物理 exFAT/USB/断电仍 NOT_RUN/BLOCKED。没有安装驱动、修改用户机器或降低沙盒。本增量是原生代码与真实 SDK 签名验证，尚不可用作工作盘。
