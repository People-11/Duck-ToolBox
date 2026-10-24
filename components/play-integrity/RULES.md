# Play Integrity 配置规则与实现边界

核对日期：2026-10-24。上游引擎固定为 `23ec56c2440c4fd1a5e893a23ef7861d6366a828`。
本文记录可复核的规则依据、兼容选择及验证边界，不提供可保证通过的设备配置。

## 判定的依据

[Google 的公开判定说明](https://developer.android.com/google/play/integrity/verdicts#device-integrity-field)
区分设备、应用和账号信号。Android 13 及以上的 Device 要求硬件支持的锁定启动及认证系统证明；
Strong 还要求最近一年内的系统及 vendor 等分区安全更新。Android 12 及以下的 Strong 没有相同的补丁新鲜度要求。
Android 13 及以上的 Basic 要求证明的 root of trust 由 Google 提供；此版本范围内的可选 Basic／Strong 标签只向通过 Google Play 安装或更新的请求应用返回。未获许可时 Device 等满足条件的其他标签仍可能返回。Basic、Play 商店认证、本地证书链检查和服务端完整性结果不能互相替代。

修改本地 SDK、日期、指纹或证书显示，不能证明真实安全更新、锁定状态、设备认证或服务端接受。
本项目不将“Pixel 全被拉黑”“首发 API 32–35 必过”“36 必须 RKP”写为确定的服务端规则。
授权社区导出中的消息 26595–26610、26681、28620、28966、29043 提供相互冲突的经验与个案；
它们没有公开服务端实现、完整测试条件或可独立验证的完整性令牌，不能据此建立通用允许／拒绝名单。
导出的原文、人员信息、密钥与附件不随源码或安装包分发。

消息 26593、26596、26639、28962 描述了无需 PIF 或关闭 PIF 的经验路径。
因此 PIF 是独立可关闭的能力，不因导入 keybox、生成指纹或应用配置而自动重新启用。
关闭不接管或关闭 Tricky Store／TEESimulator／OhMyKeymint；这些功能仍由现有适配器管理。
不根据 Key Attestation 的显示或社区经验自动判断设备已满足 Google 三种完整性标签。

## 配置语义

| 能力 | 规则与处理 |
| --- | --- |
| Build 与系统属性 | 分别配置，分别生效；对显式不一致的对应字段给出预览提示，保持用户值。迁移只补缺失的对应属性，不覆盖现有值。 |
| Build 值类型 | 按实际反射字段处理 String、int、long 和 boolean；JSON 布尔规范化后的 1／0 与文本 true／false 均可用于布尔字段。非法类型或数值只跳过对应字段并记录日志，不把无法识别的布尔值静默转为 false。 |
| 文本编码 | 配置使用有效 UTF-8，原生 prop 解析也拒绝非法字节。Java 字符串按标准 UTF-8 显式解码，避免把补充平面字符交给只接受 Modified UTF-8 的 NewStringUTF。 |
| 首发 SDK 与 board API | 普通配置和迁移不注入 25／32；可以通过字段编辑器或原文设置，缺少时提示。保留已有值。首发 API 与当前系统 SDK 含义不同；不能用首发 SDK 直接推断服务端采用哪一组 Android 版本条件。 |
| Provider | 新配置及显式迁移缺省写入 `spoofProvider=0`，不覆盖已有开关。历史配置没有该字段时保留原生兼容默认 1；界面可显式选择。此 hook 阻止 DroidGuard 读取证书链，不代表生成或增强硬件证明。 |
| 签名 | `spoofSignature` 默认关闭；仅改变进程内包信息，不能改变服务端应用许可或签名认证。 |
| Play 商店 | 指纹开关 0 关闭、1 使用配置指纹，也可指定自定义指纹；SDK 为 0 时关闭、1 对应上游兼容值 32，其余正数为目标 SDK 上限。SDK 兼容优先于指纹，不声称改变真实系统版本。 |
| 属性通配符 | 只支持前导 `*` 的后缀匹配，精确键优先；其他形式提示不支持。仅 hook 已存在属性的 `__system_property_read_callback` 路径，不创建属性，不覆盖所有原生读取 API。 |
| Canary | 按既定兼容选择保留上游映射：Pixel 10a 使用 35（上游注释声称实际 36），未知设备使用 37，board API 使用 32。发布日期来自 HTTPS 响应头，六周失效及无法取到精确补丁时的回退日期均为估计。生成时保留一致的 Build／属性补丁值，不再依据 keystore 目录注释属性项；可在字段编辑器自行移除。预览明确说明，不据此判断证书有效期或服务端接受。 |
| 导入与迁移 | GPL CLI 将配置作为数据解析，不执行 `eval`；保留未知字段和源文件，输出覆盖前保留备份。生成只准备预览，不发布配置或修改 keystore 策略。 |
| 文件权限 | 引擎自有数据目录保持私有。独立迁移或下载写入已有外部目录时保留目录权限；仅将新建目录和输出文件设为私有，不收紧共享目录的访问权限。 |
| 安全补丁联动 | 引擎只建议日期；现有 keystore 适配器持有策略映射与写入。默认不勾选联动，应用经过预览、持久备份及后端事务记录；中断后阻止再次应用，由用户确认恢复。恢复按适配器 restart_keys 报告重启需求。修改策略不等于安装更新。 |

## Root 与生命周期

| 场景 | 行为与证据边界 |
| --- | --- |
| Magisk Zygisk | 安装目录不作为启用证据；使用 daemon 传入的 `ZYGISK_ENABLED=1` 或同次启动记录。外置 provider 只检查有效模块声明及 disable/remove 标记，不据此声称目标进程已注入。 |
| 无配置 | 完整模式须先应用有效非空配置，才执行全局兼容脚本；明确保存 scripts-only 后可独立运行。启用但缺配置时不回落执行旧敏感属性处理器；boot hash 仍由原处理器持有。 |
| ROM 遮罩 | 仅允许受支持分区 app／priv-app／overlay 下单个条目；根目录、父目录、overlay 通配符和 config 目录均拒绝。CLI 保存与脚本运行前均校验，包括展开后的名称。 |
| DenyList | 保留上游自动管理：完整模式在强制名单开启时移除 GMS／Play 商店；scripts-only 模式添加。CLI 按包名和进程记录原值、预期值及未完成操作，写入前保存，写后读回。关闭仅恢复可核对的自有改动；外部编辑保留并提示。Magisk 在强制名单开启时添加条目可能终止相应进程，恢复操作也可能触发。未记录的历史改动不推测原值。 |
| 早期启动 | 在模块挂载之前准备 ROM 冲突遮罩及早期 OEM 属性。使用 `resetprop -n` 避免触发 init 属性服务；不联网、不等开机完成。 |
| 开机完成 | 延后处理 verified boot、锁定状态等 OEM 敏感属性，保持上游避免指纹识别／显示模式故障的时序。 |
| KernelSU late-load | 由 `late-load.sh` 代替早期阶段；当前 KernelSU 源码随后加载属性并调用 metamodule 挂载，再执行后续阶段。挂载实现与 OEM 行为须实机验证。 |
| SELinux | 保留 permissive 时限制 enforce/policy 文件读取权限的兼容行为；不是切换内核 enforcing。记录原始权限，关闭时仅恢复未被外部修改的权限；持久属性恢复记录对引号等字符作 shell 数据转义，按上游写回原值，可能覆盖后续对相同持久属性的修改；此行为在关闭界面明确告知。 |
| 独立关闭 | 先发布停用令牌，再取消生成任务和恢复有记录的兼容状态；非法配置、写锁忙或待恢复事务不阻止停用标记。显式启用只确认当前停用令牌，较早的启用请求不能覆盖后来的关闭。原生 companion、后台任务与 Shell 均检查此状态。已注入进程需重启，已有挂载及全局属性需重启设备。清理失败保留记录并提示重试，不重新启用。 |
| 停用令牌与通信 | 空停用令牌不能作为有效启用确认，Rust、原生 companion 与 Shell 保持一致。companion 帧头、DEX 与配置传输共享两秒单调时钟截止时间；使用非阻塞 socket，慢速分段数据不会重复延长窗口。接收失败清空输出，超时不作为注入证据。 |
| 发行架构 | 当前管理程序及引擎 CLI 为 arm64，注入库包括 arm64 与 ARMv7 进程版本；ARMv7 注入库不表示支持 32 位系统。安装器在准备运行数据之前拒绝不匹配的系统架构。 |
| 禁用／冲突 | 禁用或有活动 PIF／USNF 时跳过注入及引擎属性脚本；早期清理自有遮罩并恢复持久设置。现有属性处理器继续执行原路径及 boot hash 职责。全局修改及已运行进程的 hook 需要重启清除。 |

## 固定源码依据

- [Magisk a51dc10f：模块脚本与挂载顺序](https://github.com/topjohnwu/Magisk/blob/a51dc10f88ea3a8aac7fa02e95ae7589a034730b/native/src/core/module.rs)：先执行模块 post-fs-data，再处理 system.prop 与模块树。
- [Magisk a51dc10f：脚本环境](https://github.com/topjohnwu/Magisk/blob/a51dc10f88ea3a8aac7fa02e95ae7589a034730b/native/src/core/scripting.cpp)：Zygisk 启用信息由 daemon 显式传入。
- [Magisk a51dc10f：Zygisk API](https://github.com/topjohnwu/Magisk/blob/a51dc10f88ea3a8aac7fa02e95ae7589a034730b/native/src/core/zygisk/api.hpp)：强制卸载仅作用于进程挂载命名空间，不表示完整性通过。
- [Magisk a51dc10f：DenyList CLI](https://github.com/topjohnwu/Magisk/blob/a51dc10f88ea3a8aac7fa02e95ae7589a034730b/native/src/core/deny/cli.cpp)及[名单操作](https://github.com/topjohnwu/Magisk/blob/a51dc10f88ea3a8aac7fa02e95ae7589a034730b/native/src/core/deny/utils.cpp)：输出为 `package|process`，添加名单可触发进程终止。
- [KernelSU 3352209f：启动阶段](https://github.com/tiann/KernelSU/blob/3352209f44fc9c5f3ec6dff228080118433cb72c/userspace/ksud/src/init_event.rs)及 [late-load](https://github.com/tiann/KernelSU/blob/3352209f44fc9c5f3ec6dff228080118433cb72c/userspace/ksud/src/late_load.rs)。
- [AOSP frameworks/base 1cdfff55：Build](https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/os/Build.java)、[SystemProperties JNI](https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/jni/android_os_SystemProperties.cpp)及 [AndroidKeyStoreSpi](https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/keystore/java/android/security/keystore2/AndroidKeyStoreSpi.java)。
- [AOSP bionic 731631f3：属性读取](https://android.googlesource.com/platform/bionic/+/731631f300090436d7f5df80d50b6275c8c60a93/libc/system_properties/system_properties.cpp)：同步 callback、cookie 与只读属性长值语义；原生 Get 另走 Read。
- [AOSP system/core a3b721a3：属性服务](https://android.googlesource.com/platform/system/core/+/a3b721a32242006b59cb12bd62c9133632af3a2d/init/property_service.cpp)：ro 属性 write-once 及派生属性逻辑。
- [Android NDK：JNI 字符串说明](https://developer.android.com/ndk/guides/jni-tips#utf-8_and_utf-16_strings)：NewStringUTF 接受 Modified UTF-8；标准 UTF-8 输入需要显式解码。

## 可复现验证与限制

独立 Rust 测试覆盖未知字段、API 值保留、Provider 默认值、只读规则诊断、shell 元字符迁移、Zygisk 误判、超时与取消。
生成进程独立持有子孙进程组；取消、超时、输出超限、异常及正常完成后均终止残留进程，候选配置不会自动发布。
配置锁在所属对象结束时显式释放，不因 fork／exec 期间暂存的文件描述符副本延长锁期。
`python3 scripts/test-play-integrity-scripts.py` 使用隔离命令替身验证持久属性恢复、停用标记及 OEM 前后阶段；DenyList 模式与条件恢复由独立 Rust CLI 测试覆盖。
`python3 scripts/test-play-integrity-native.py` 检查原生配置、慢速与阻塞 companion 边界，并通过启用 CheckJNI 的主机 JVM 验证实际 UTF-8 字符串转换。
`python3 scripts/test-play-integrity-java.py` 在主机 JVM 验证实际 Build 值转换，包括规范化布尔值、数值边界和非法值。
WebUI 的真实后端契约与浏览器流程覆盖预览、字段编辑、导入、应用失败恢复与多语言。

这些检查不验证 Google 服务端判定。当前没有连接设备；Zygisk 注入、属性 hook 覆盖、OEM 启动、
SELinux 权限影响、各 root／挂载实现、实时 Canary 元数据与实际进程重启仍需真机验收。
