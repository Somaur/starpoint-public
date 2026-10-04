# Somaur 分发维护

默认开发分支为 `android`，上游基线为 `6741e0724a2304109ccfd7440fa7bdb8ad13b9cf`。`main` 和 `ios` 保留上游内容，iOS 尚未移植本次修复。

## 自动化

- `Source checks`：推送 Android 分支或创建 PR 时，构建 TypeScript，验证数据契约，运行发布脚本测试和全部 Rust 测试。
- `Android distribution`：推送 `somaur-v*` 标签时先运行相同检查，再交叉编译 ARM64 Rust/JNI、获取固定版本输入、修补并签名客户端和服务、验证每个 CDN 文件、生成 1 GiB 分片及完整校验清单，上传为预发行草稿。
- 草稿经实际安装验证后再公开。手动运行工作流时须选择已存在的 `somaur-v*` 标签；在分支上运行只进行源码检查。

签名使用仓库 Actions Secrets `ANDROID_KEYSTORE_BASE64` 和 `ANDROID_KEYSTORE_PASSWORD`，别名为 `local-repair`。私钥在仓库之外保存并另行备份；后续版本必须使用同一密钥才能覆盖安装。工作流不会上传私钥和密码。

## 固定输入

`upstream-android.json` 固定上游 0.0.16 Android APK、全部分片、基础前缀和 SWF 的 SHA-256。构建下载总量约 11.4 GB。`comics.json` 固定漫画归档及其中 148 个文件的校验值，归档存放在本 Fork 的 `distribution-inputs-v1` Release。

游戏和大资源不进入 Git 历史。最终 Release 的所有分片存放在本 Fork，朋友安装不依赖上游下载地址。维护者重新构建仍依赖固定的上游基础包；需要脱离上游时可把相同分片镜像到自己的资源 Release，仅修改 `upstream-android.json` 的 URL，不改变校验值。

## 构建边界

这个流程重编译修改过的 Rust/JNI 代码，并沿用与本地实测版本相同的上游启动器及游戏基线。它不是从官方游戏源码重新编译 APK。`AndroidGachaOddsPreloadPatch.java` 保留语义定位工具；发行流程对固定 SWF 校验后应用同一个 1 字节补丁，再核验修改后的 SHA-256。

漫画位于 CDN 的 `local-comics/`；个人服务优先使用用户显式导入的目录，否则读取随包漫画。原来的单话内嵌资源仍用于没有完整漫画的开发环境。漫画来源为 World Flipper Jukebox 存档，语言限制见 Release 说明。

## 本地重现

安装 Rust 1.78.0、Android NDK、SDK build-tools、Java、Python 3.11+；按工作流编译 native 库后执行：

```text
python scripts/distribution/package_android.py --work build/package --output build/release --version somaur-v0.1.0 --library PATH_TO_LIB --build-tools PATH_TO_BUILD_TOOLS --keystore PATH_TO_PRIVATE_KEY
```

密码通过环境变量 `ANDROID_KEYSTORE_PASSWORD` 提供。可用 `--baseline` 和 `--comics` 指向已下载的完整 APK 及漫画 ZIP；输入仍会校验。输出目录必须尚不存在。

每个下载清单记录对应源码提交、基础 APK、资源清单、整包和每个分片的摘要。保留发行标签，修订版本使用新标签，避免覆盖朋友已下载的资源。
