# SpaceLens 空间镜

SpaceLens 是一个 macOS 本地磁盘空间分析工具，帮助你查看目录占用、定位大文件，并通过可选的 DeepSeek AI 解读了解文件用途。

技术栈：Rust + Tauri 2 + React / TypeScript + SQLite。当前为核心功能可运行的开发版本；Windows 适配、完整性能验收和正式签名发行尚未完成。

## 启动与构建

开发需要 macOS、Node.js 24 LTS、Rust stable 和 Xcode 命令行工具。已提供 `.nvmrc`；使用 nvm 时可在项目目录执行 `nvm use`。若尚未安装对应版本，先执行 `nvm install`。

取得源码后，先在项目目录安装前端依赖：

```sh
npm ci
```

Rust 可以使用系统已有工具链，也可以按下一节安装在项目内。然后运行：

```sh
npm run desktop         # 开发模式，自动启动前端和原生窗口
npm run desktop:build   # 构建优化后的 macOS .app
npm run build           # 前端类型检查及打包
npm test                # 前端计量和视图状态测试
npm run test:core       # Rust 扫描与查询测试
npm run test:native     # DeepSeek 请求与元数据预览测试（本机模拟服务）
```

构建输出：`target/release/bundle/macos/SpaceLens.app`。该应用可独立运行，不需要启动前端服务，也不需要运行机器安装 Rust。当前产物为 Apple Silicon 架构，最低系统配置为 macOS 14；实际运行测试系统为 macOS 26.5.1。未完成 Developer ID 签名与公证，不作为公开发行包。

调试构建命令为 `npm run tauri -- build --debug --bundles app`，输出至 `target/debug/bundle/macos/SpaceLens.app`。`npm run dev` 只有浏览器界面预览，真实扫描须使用桌面应用。

## 项目内 Rust

仓库不包含 Rust 工具链、依赖缓存或编译产物。已验证版本为 **Rust 1.99.1 / aarch64-apple-darwin**，采用 minimal 配置并安装 rustfmt。

可选：在 macOS 项目根目录执行下列命令，使用官方 rustup 将工具链安装到本项目。`--no-modify-path` 保持 shell 启动配置不变。

```sh
mkdir -p .local-tools
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o .local-tools/rustup-init.sh
CARGO_HOME="$PWD/.local-tools/cargo" RUSTUP_HOME="$PWD/.local-tools/rustup" \
  sh .local-tools/rustup-init.sh -y --no-modify-path --profile minimal --component rustfmt
```

本地目录约定：

| 路径 | 用途 |
| --- | --- |
| `.local-tools/rustup/` | 工具链与 rustup 配置 |
| `.local-tools/cargo/` | Cargo、rustup 入口与依赖缓存 |
| `target/` | 两个 Rust crate 共用的编译产物 |
| `scripts/run.mjs` | 为子进程设置本项目的 Rust 环境 |

npm 的桌面及 Rust 测试命令优先使用项目内工具链；没有本地工具链时，使用系统 PATH 中的 Rust。直接在普通终端输入 `cargo` 不会自动启用项目内工具链。可通过统一入口检查和管理：

```sh
node scripts/run.mjs rustc --version
node scripts/run.mjs rustup show
node scripts/run.mjs cargo fmt --manifest-path src-tauri/core/Cargo.toml
node scripts/run.mjs cargo fmt --manifest-path src-tauri/Cargo.toml
# 需要升级本项目工具链时手动执行：
node scripts/run.mjs rustup update stable
```

依赖缓存齐全时，可用 `npm run test:core -- --offline` 离线测试，或 `CARGO_NET_OFFLINE=true npm run desktop:build` 离线构建。首次安装和依赖更新需要网络。工具链、缓存及编译目录已在 `.gitignore` 中排除。

## 已实现功能

- 选择当前用户目录、系统选择器指定的文件夹或本地挂载卷；单次扫描、进度显示和取消。
- 实际读取文件元数据，区分逻辑大小、已分配空间、卷容量；不读取文件正文、不删除文件。常规扫描不联网，可选 AI 解读按下述流程发送选中条目的元数据。
- 空间地图、目录下钻与面包屑、大文件排行、搜索、稳定游标分页、条目详情。
- 硬链接按文件身份去重；保留原始大小和归属路径。稀疏文件按文件系统报告的分配块数计量。
- 未知大小、访问失败、策略跳过分开处理；权限说明及 Finder 定位、复制路径。
- SQLite 本地索引、扫描中保留上次结果、可选择取消后的部分结果、异常中断恢复、两代记录保留。
- 容量单位和默认口径持久化；启动恢复已有结果；扫描中退出确认。
- DeepSeek API Key 钥匙串保存、文件及文件夹旁的 AI 解读按钮、发送信息预览、用途解释和请求失败提示。

应用数据位于 `~/Library/Application Support/com.spacelens.desktop/`，其中包含扫描路径元数据。这与源码和项目内 Rust 工具链是不同目录。扫描会排除自身索引目录。

## DeepSeek AI 解读

1. 打开「偏好设置 → DeepSeek AI 解读」，输入 API Key 并保存。Key 存入 macOS 钥匙串，不写入 SQLite、浏览器存储或项目配置；界面不回显已保存的值。
2. 扫描后，在文件或文件夹所在行点击「AI 解读」，也可使用右侧详情中的「AI 解读用途」。
3. 查看名称、路径、类型、逻辑大小、分配空间及完整性状态，然后点击「发送并解读」。这一步才调用 DeepSeek，费用由你的 API 账户承担。

当前采用官方 `https://api.deepseek.com/chat/completions` 接口、`deepseek-flash` 模型、非思考模式。接口根据 2026-11-11 的 [DeepSeek 官方文档](https://api-docs.deepseek.com/) 和 [Chat API 参数](https://api-docs.deepseek.com/api/create-chat-completion) 核对。

仅发送预览中的扫描元数据及固定用途解读提示，不读取文件正文、不列举或发送子项清单。用户主目录前缀替换为 `~`，其余路径仍可能含私人信息。请求不自动触发、不自动重试；单次最长等待约 60 秒，响应上限 64 KiB，结果仅保留在当前弹窗。关闭弹窗不会保证服务端停止已发送请求。AI 回答以纯文本显示，无法执行其内容，也没有清理操作。

保存 Key 只验证输入格式，不代表已验证账户有效性。认证失败、余额不足、限流、超时分别提示。使用「移除 Key」可删除本机保存的凭据，不会撤销 DeepSeek 平台上的 Key。

AI 功能验证：新增 5 项界面测试、7 项 Rust 测试通过，覆盖未配置引导、主动发送、重复点击、过期结果、密钥输入、预览单次消费、路径脱敏、模拟 HTTP 成功与错误、响应大小和超时。真实 `.app` 已验证钥匙串保存、清空输入、重新读取、移除和发送预览；所用虚构测试 Key 已移除。未使用真实 API Key 调用 DeepSeek；真实账户、账单和模型回答质量需填入 Key 后验证。

测试依赖已更新至 Vitest 4.1.12；2026-11-11 安装后的 npm audit 为 0 个已知漏洞。当前 AI 版本的 Release `.app` 约 13.73 MiB。

## 验证记录（2026-11-11）

| 验证 | 结果及边界 |
| --- | --- |
| TypeScript / Vite | 类型检查及前端生产打包通过 |
| Vitest | 10 项通过：原有 5 项计量/视图测试及新增 5 项 AI 界面测试 |
| Rust 常规测试 | 1 项单元测试及 8 项集成测试通过；覆盖稀疏文件、硬链接、权限缺口、分页分组、取消、恢复、设置、路径身份等 |
| 受控规模测试 | 另行运行 2 万文件测试通过，扫描约 0.91 秒；100 个目录下的空文件，不含样本创建时间，不代表实际整盘性能 |
| 实际原生应用 | 系统目录选择器、扫描、地图下钻、口径切换、去重排行、文件名搜索、Finder 定位通过 |
| 受控扫描样本 | 8 个普通文件路径、7 个唯一文件，逻辑累计 540 MiB、分配累计 28 MiB；512 MiB 稀疏文件分配量为 0，符号链接按策略跳过 |
| 优化构建复核 | Release `.app` 成功构建并启动，上次结果恢复正常；复扫识别到 Finder 新建的 `.DS_Store`，文件条目增至 9 个，分配量显示为 28.01 MiB |

真机验证中已修复备份快照被误列为独立磁盘、细小地图图块溢出、详情将完全未知贡献显示为零、旧结果错误链接指向新扫描等问题。加入 AI 功能前的基础版本 `.app` 约 10.07 MiB；AI 版本大小见上一节。

规模测试默认忽略，需显式运行：

```sh
node scripts/run.mjs cargo test --manifest-path src-tauri/core/Cargo.toml scan_twenty_thousand_files -- --ignored --nocapture
```

## 尚未完成的验收与限制

- Windows 元数据、文件身份、路径及系统操作适配尚未实现；当前源码不能直接构建 Windows 版。
- 尚未完成百万文件、峰值内存、最小窗口与完整可访问性验收，不能宣称达到设计中的全部性能目标。
- 外置卷断开、满盘、数据库损坏、云端占位、压缩文件及资源分叉等系统场景仍需专门验收；当前权限缺口测试不能替代 macOS 全套隐私授权验证。
- 前端会丢弃过期查询响应；数据库查询的主动取消及执行超时仍待完善。大范围子串搜索尚未实测性能。
- 硬链接详情面板最多展示 10 条路径并提示更多路径；完整别名分页交互尚未实现。
- 仅扫描本地且可访问的选定范围，不提供完整启动卷 `/` 或 `/System/Volumes/Data` 扫描；不跨卷、不跟随符号链接、不扫描挂载快照或网络共享，目录深度上限为 128。
- 不计算 APFS 克隆或快照的独占块；已分配空间不能当作删除后可释放空间。各目录硬链接分摊可能随遍历顺序变化。

## 代码位置

| 路径 | 职责 |
| --- | --- |
| `src/` | 页面、地图、IPC 调用、计量展示 |
| `src-tauri/src/main.rs` | Tauri 命令、原生选择器、系统操作与退出处理 |
| `src-tauri/src/ai.rs`、`src-tauri/src/ai/tests.rs` | DeepSeek 请求、钥匙串、预览校验及模拟服务测试 |
| `src/components/DeepSeekSettings.tsx`、`AiExplanation.tsx` | Key 设置、元数据预览及 AI 结果弹窗 |
| `src-tauri/core/src/platform.rs` | macOS 文件系统与路径操作 |
| `src-tauri/core/src/scan.rs` | 有界遍历、批量写入、归属与聚合 |
| `src-tauri/core/src/store.rs`、`schema.sql` | SQLite 数据与恢复 |
| `src-tauri/core/src/query.rs` | 分页、过滤、地图、详情 |
| `src-tauri/core/tests/analysis.rs` | 真实临时目录集成测试 |

产品行为和后续验收继续以 `产品与技术详细设计文档.md` 为基线。`产品交互原型.html` 保持原样。

## 隐私与提交约定

源码中的 API Key、用户名及路径示例都是测试占位数据。使用自己的 DeepSeek Key 时，请通过应用的偏好设置保存到 macOS 钥匙串。

`.gitignore` 排除了工具链、依赖、构建包、环境配置、私钥和签名材料、扫描数据库及日志。不要强制提交这些文件。提交前用 `git diff --cached` 检查实际暂存内容；`.gitignore` 不会移除已经被 Git 跟踪的文件。

反馈问题时，请隐去截图和日志中的私人文件路径、用户名及 API Key。扫描数据库位于应用数据目录，不属于源码交付物。

## 许可证

本项目采用 [MIT License](LICENSE)。允许使用、修改、商业使用和分发；分发源码或其重要部分时，须保留版权声明和许可证。第三方依赖遵循各自的许可证。
