# M0-K：FSVolume 操作接入（直接 SDK 合同验证，未挂载）

在 M0-J 的 Xcode App/Extension 工程内新增真正的 FSVolume 子类，以及每卷串行请求队列和共享 Rust/C ABI 会话。实现 lookup/create、硬链接、删除、原子 rename、open/close/reclaim、读写/截断、容量、后端同步及带 cookie/verifier 的目录枚举回调。不是宿主目录透传，也不创建另一份系统盘工作树。

## 身份与生命周期

Core ROOT=1 映射 FSKit rootDirectory=2，其他对象 ID 加一，避免与 FSKit parentOfRoot=1 冲突。同一对象的硬链接共享同一个 FSItem；每卷 UUID tag 和缓存实例校验拒绝跨卷/已回收项。文件在 lookup/create 时持有 Core 句柄，取消链接或替换后仍能读写，直到显式 reclaim 才释放。open/close 使用 FSKit 聚合剩余 mode，禁止在 close 时新增权限；仍有打开项或未 reclaim 的文件身份时 deactivate 返回 BUSY。目录打开期间拒绝 rmdir/替换；目录完整跨系统语义尚须真卷验证。

请求串行执行，原生存储同步排在此前已入队的操作后。deactivate 在释放 owner 前执行后端屏障；失败不强行释放。unmount 的 SDK reply 没有错误参数，只确认通知，不伪称安全弹出或绕过后续 deactivate。业务成功后的维护警告单独记录，不把已提交的写入、创建或改名改报为回滚。

## 名称与枚举

既有 ABI 新增 pw_lookup_entry，返回 Core 自己比较后的原始拼写，避免 Swift 重写 Unicode/大小写规则或根据硬链接对象 ID 误选别名；小缓冲只修改 required length。pw_parent_directory 将根的父目录限定在卷内；不会访问 backing 宿主父路径。pw_dir_seek 只在同一受限目录快照内定位，越界不移动位置。

枚举 verifier 使用唯一原生 cursor token，cookie 是快照位置。packer 满时不丢掉未打包项，下一次从 cookie 重新定位；名字空间变动使现有 verifier 明确过期。同一未变目录的新 listing 复用不可变快照；每个请求按自己的 cookie 定位。全卷最多保留 32 个快照，LRU 淘汰前先关闭原生 cursor，被淘汰 verifier 明确 ESTALE，可从零重新开始，避免重复/放弃 listing 永久耗尽原生 64 个 cursor 上限。目录 reclaim/会话关闭释放 cursor。当前每快照 100,000 项及 4096 个缓存 FSItem 仍是实验上限；这不是完成大规模工作盘兼容。

## 不开放的能力

当前 v1 格式没有持久化 mode/uid/gid/时间/xattr 或卷 UUID。只返回已实现的 type/size/fileID 和普通文件 linkCount；其他属性在变更前明确 ENOTSUP，不消费或伪造字段。带创建属性的请求先拒绝；符号链接未实现。checkAccess 始终拒绝，activate/mount 和 unary probe/load 继续拒绝。session-only UUID 不宣称持久卷 ID。不能把这些拒绝改成世界可写或假成功来启动演示挂载。

当前开发目标使用实际 Xcode 26.6 / macOS 26.5 SDK 的 Operations 协议；Apple 后续 macOS 27 Handler/FSContext 属于另一套版本，不能靠网页最新签名混用。后续应在固定 SDK/OS 矩阵内补权限、元数据、真实缓存及签名授权。

## 验证分层

Rust/C 回归覆盖保留别名拼写、根父边界和同一 cursor 重试/越界。macOS 编译实际 FSVolume 协议，并用真实 SDK 对象直接调用创建、写入、属性、链接、rename、删除、打开项保护、同步和生命周期；枚举分页用同一 session 打包算法测试。SDK 的 FSMutableFileDataBuffer/FSDirectoryEntryPacker 没有公共构造器，测试不伪造它们：读回走同一 session 的真实 C ABI，packer 的实际系统分发仍 NOT_RUN。准确 PASS 以对应 PR/CI 源 SHA 为准。

没有签名安装、启用扩展、Finder mount、Codex 或物理 exFAT 验收。Windows WinFsp 的权限/共享/缓存/硬链接阻断不因这些 macOS 回调解决。

保留首轮 native/bundle run 40112544717 / 40112544675 的 Swift Int32/导入常量 Int 模式匹配失败，修正后再验证。代码审查另指出重复 listing 可能耗尽 cursor，已加入快照复用/LRU，并以 256 次不变目录枚举、80 个不同目录扫描、淘汰后 ESTALE 及从零重新开始回归验证；准确通过状态以修正源码的 CI 为准。

后续 M0-W（2026-10-28）：[新范围](IMPLEMENTATION-M0W.zh-CN.md) 将 FSKit 自有游标改为一起捕获名字/属性。新 initial listing 原子替换旧快照并返回新 verifier，取代本阶段的同 verifier 复用；非零 verifier 的旧页仍只重放。32自有LRU/共64cursor不泄漏规则保留，增补dots/非法cookie；实际packer分发与挂载仍未验证。
