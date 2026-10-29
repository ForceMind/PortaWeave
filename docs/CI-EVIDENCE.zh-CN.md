# 主线 CI 证据审计

2026-10-29。`tools/ci_evidence.py` 审计保存的 GitHub Actions MAIN push run/jobs JSON，不联网、不修改检查、不启动或取消作业，也不认证输入的来源。

输入为 `{"source_commit":"完整SHA","workflows":[{"run":RUN_JSON,"jobs":[JOB_JSON]}]}`。采集者必须自行从GitHub读取完整分页、同一提交的当前运行和job列表；选择哪些workflow、快照是否最新及其真实性不由本工具保证。输入不是签名证明，不能拿自行伪造的JSON证明CI成功。

```sh
python tools/ci_evidence.py snapshot.json --sha FULL_SHA --workflows 5 --jobs 14
```

默认对应现有main五工作流/14项作业。检查完整SHA、main/push、不同workflow/run/job ID、job所属run/source、作业完整数量及完成状态；所有run/job须completed/success，队列中、运行中、取消、失败或跳过的job均拒绝。步骤可有平台条件造成的skipped，但须已完成且至少一步成功；失败/缺失步骤拒绝。退出码0只表示给定快照通过结构和状态检查，1表示拒绝。始终输出product_certified=false；不证明测试内容、权限、真实挂载或物理介质安全。

实测：6个新用例加原有9项Python测试共15项通过。用GitHub当前真实数据采集到的ea33b7e主线App构建单工作流/单job快照通过；包含正在补跑的五工作流快照拒绝，报告未完成和job数量缺失。单项通过不代表全套通过。

上轮ea33b7e主线首轮core/native/handover的macOS job分别122966569599/122966569354/122966856519已cancelled，交接Windows/return为skipped，不能改报PASS。取消原因未获得可核验说明，不猜测。2026-10-29仅补跑三套未完成流程，保留首轮记录、准确source不变；补跑结果必须重新读取。详细准确SHA和CI结果见本增量PR；真实挂载、驱动/签名授权、OS权限/共享/锁/同步及卷内Codex验收仍BLOCKED/NOT_RUN。

补跑实际结果：native和三平台交接attempt2成功；core attempt2在macOS默认构建的range_locks夹具创建时报EXISTS（-3），不是全绿。夹具只用进程ID和wall-clock时间戳，多线程可能同tick碰撞。新增进程内AtomicU64序号，避免并发路径重名；保留15项Python实测结果，Rust本地工具链缺失，修正后Rust结果必须由准确新source CI验证，不拿旧源码通过替代。
