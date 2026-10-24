# M0-M：FSKit v2 原子属性设置与带元数据创建

使用 M1-I 单事务 primitive 实现 v2 `FSVolume.setAttributes` 的 size + uid/gid/mode/birth/access/modify time 混合请求。含 size 时先验证所有参数并确认 write open mode，再调用一次 `pw_file_truncate_with_metadata`，不拆成两个提交。metadata-only 请求调用同一核心 metadata setter。成功后只消费实际提交的字段，拒绝或失败时不消费；changeTime 由核心维护，不接受调用者设置。

v2 `createItem` 原子存储明确 uid/gid/mode，字段缺省时沿用核心实验规则：父 uid/gid + file 0600 / directory 0700；没有猜测当前操作系统 caller、umask 或 setgid 继承。创建时的 size、时间、flags 等未支持字段仍在 mutation 前拒绝。v1 元数据设置/带属性创建仍 unsupported，不改写旧容器。v2 SDK capability 现在反映可以设置存储的 mode/root times；v1 保留不支持。

纳秒在 Swift 转换为 C struct 前检查非负且小于 1e9，mode 必须在 07777 范围内；未知/不支持字段先拒绝。混合截断后的显式 mtime 优先、ctime 自动、file setid 清除与一次 generation 由核心事务保障，不在 Swift 用临时回滚拼接。原件/维护状态规则不变。

实际 SDK 合同增加：只改 gid、成功混合截断/mode/精确 mtime/一次 generation/消费位、未打开写模式拒绝且不消费、非法纳秒无变更、size + flags 拒绝且不截断、明确创建所有者和 mode、带不支持创建时间请求不留下名字。保留 v1 的 unsupported 回归及所有身份/链接/分页压力测试。准确 PASS 取对应源码 SHA 的 macOS SDK/native 与 App/Extension CI；Work Linux 不提供 SDK 编译证据。

这里的调用仍仅在实验性直接 SDK 合同中验证，**不提供 OS caller 认证、ACL/目录遍历/粘滞位授权或安全执行证明**。checkAccess、unary load、activate、mount 继续拒绝；尚未启用安装后的系统回调。不允许据此打开用户卷。Windows 权限/共享/同步、符号链接/xattr/ADS、实际挂载的 Codex/Git/依赖/编译/IDE/SQLite、正常沙盒负向以及物理 exFAT/USB/断电仍未完成。
