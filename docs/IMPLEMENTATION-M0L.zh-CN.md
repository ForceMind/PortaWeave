# M0-L：FSKit 读取 v2 元数据与持久卷身份

在 M0-K 真实 FSVolume 子类与 M1-H v2 存储基础上，将 v2 UUID 接入 `FSVolume.Identifier`，并在 getAttributes / 同一目录打包算法返回实际 uid/gid/mode、birth/access/modify/change time。纳秒和负秒通过整数转换为 SDK timespec，不经浮点 Date。保留每会话独立随机 tag 来拒绝旧 FSItem，不能把持久卷 UUID 复用成跨会话对象授权。

v1 的 UUID 仍仅限会话，缺失元数据请求仍 ENOTSUP；不补零、不升级原件。只有 v2 声明 persistent object IDs。未实现的 flags、文件 parentID、备份时间、分配大小等继续拒绝，不设置有效位。目录枚举在分配快照前检查请求范围，复用 getter 的实际填充方法；名字快照不承诺冻结之后的元数据。

本增量提供属性读取。setAttributes/createItem 对权限/所有者/时间等仍先拒绝；大小与不支持的属性混合时，不先截断或消费部分请求。`doesNotSupportSettingFilePermissions` 仍 true。权限位可读不等于有效授权或执行成功；access、unary load、activate、mount 继续关闭。Windows ACL/共享/缓存与完整 flush 未因本增量改变。

实际 SDK 合同新增 v2 UUID/重开 FSVolume ID 相等、不同会话 tag、uid 最大值、负秒与纳秒、字段有效位、未知属性拒绝、混合设置无副作用、写入后的 setid 清除及时间变化、硬链接与枚举属性、access 拒绝测试。继续运行 v1 全部回归。准确编译/运行结果以对应源码 SHA 的 native/bundle CI 为准；Work Linux 没有 macOS SDK，不把文本检查声明为 Swift 编译。

直接 SDK 对象、C readback 与打包闭包不等于系统回调分发；没有构造假的 FSMutableFileDataBuffer/FSDirectoryEntryPacker，没有安装扩展或挂载。真实 FSKit/WinFsp、Codex/开发工具正常沙盒和越界负向测试、exFAT/USB/断电仍 NOT_RUN/BLOCKED。
