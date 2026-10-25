# M0-R：WinFsp 普通文件 SetBasicInfo 时间合同

日期：2026-10-26。继续已有 Windows Dispatcher 与固定 WinFsp 2.1 头文件（`ddca7bd5481857a65ba552f643b8776fd070836f`），增加真正的 `SetBasicInfo` 回调签名和 `pw_windows_file_set_basic_info`。尚未安装驱动或创建系统挂载。

## 支持的实验请求

普通 v2 policy file 配合显式受信任的当前属性凭据，可在一次元数据事务设置 creation/access/write 时间。FILETIME 保留精确 100 ns ticks，转换为核心的有符号 Unix 秒和纳秒；回执继续使用 M0-N 的纯提交前投影。其他未修改字段若无法投影，也必须在 commit 前拒绝整个请求，不能先改时间再报告失败。

按固定头文件约定，`INVALID_FILE_ATTRIBUTES` 表示不改 DOS attributes，时间 0 表示该字段不改。可设置的非零 ticks 范围为 `[1, INT64_MAX]`；因此 FILETIME epoch 的 tick 0 只能由其他明确接口设置，本回调中 0 仍是 unchanged。`-1/-2` 等超范围时间控制值不当作实际日期、不伪造自动时间控制。支持请求不改变文件内容、大小、执行位、owner/group 或逻辑 allocation。

以下全部在修改前明确拒绝：任何 DOS attributes 请求（包括 NORMAL/0）、非零 ChangeTime、时间范围越界/控制哨兵、非零保留字段、无效输出/输入、管理/外来/失效 token 或当前属性权限不足。ctime 仍由核心维护；不声称 Unix mode 已映射 Windows readonly 或 ACL。全零时间 + unchanged attributes 是观察性的 no-op，保留 generation/ctime。

## 身份边界

测试 `PW_PROBE_FILE` 的 `metadata_caller` 只能指向测试显式提供、整个调用期间有效的受信任声明；NULL 直接拒绝，不默认 uid 0 或服务进程身份。真实 `Open` 和 security 回调仍拒绝，不制造这种上下文。该模式规则不是 Windows SID/ACL/WRITE_ATTRIBUTES 授权；未来原生接入需验证 caller、ACL 与授权生命周期，不能把测试声明用于不可信 IPC。

回调成功后只复制已经完成的投影，不二次查属性或执行可失败转换。后续维护健康状态与已成功业务分开，实际 commit 错误仍可能结果不明。现有 allocation 请求、Flush、硬链接后端缺口与挂载限制均不改变。

## 验证层级

原生库新增 FILETIME 反向转换单元 1 项、ABI 集成 4 项（含 M1-N 通用 C 接口）。覆盖正负 epoch、最小非零/最大 ticks、所有字段混合拒绝、no-op、read-only owner、换主拒绝、旧 unlinked inode、其他字段投影失败回滚、输出哨兵、原内容、重开和 token 生命周期。核心故障回归见 M1-N。

Windows CI 在 MSVC `/WX` 下编译实际 `SetBasicInfo` 函数表并直接调用，输出 `WINFSP_BASIC_INFO_CONTRACT_OK`；Dispatcher 自检也验证函数表和仍关闭的入口。准确源码 CI 通过后才记为运行成功。C/SDK 合同、App/Extension 构建和 artifact 交接不等于驱动挂载、Windows ACL/共享/缓存/同步或 Codex 沙盒通过。

真实挂载仍需已授权且具备 WinFsp runtime/驱动的隔离环境；macOS 签名/启用、有效身份与真卷开发工具测试也仍 BLOCKED/NOT_RUN。没有修改用户电脑、安装驱动、放宽沙盒或操作物理盘。

## 固定 API 依据

- WinFsp 固定 SetBasicInfo 声明与字段 sentinel 文档：https://github.com/winfsp/winfsp/blob/ddca7bd5481857a65ba552f643b8776fd070836f/inc/winfsp/winfsp.h
- WinFsp 教程：https://winfsp.dev/doc/WinFsp-Tutorial/ 。其中共享检查由 WinFsp 执行，后续必须通过真实驱动证明；不额外虚构一套用户态 share mode 通过结论。Cleanup 之后仍可能有 lazy-writer I/O，不能把 Cleanup 直接当成最终 Close。
