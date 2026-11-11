# Personal Agent

> 本地优先、记忆可审计、会跨对话「做梦」的个人 Agent。

**`Personal Agent` 是暂定名**，后续可能调整。代码与目录统一用中性的 `pa` 前缀，改名时不需要动路径。

聊天、干活、写代码、学习都在**同一个对话**里完成。
记忆不是一堆 Markdown 文件，而是一份**你能读、能改、能忘**的自传。

**状态：v0.1 可运行原型。** 模型链路已打通，记忆层尚未接入。

---

## 为什么做这个

上下文窗口越来越大，「记得你」这件事却没变好。开一个新对话，一切归零；想接着上次，只能自己贴摘要。

现有的 Agent 记忆大多是 `MEMORY.md`：机器能读，人读不下去；看得见结论，看不见它为什么这么记；想删一条，得先找到它散落在哪。

我们想要的是像人一样的记忆：跨对话地记住有意义的事，而且这份记忆属于你——可以翻看，可以改，可以彻底忘掉。

## 三个主张

**一个对话。** Chat、Work、Codex、Learning 是同一个工作台的四种用法，不是四个入口。不切 tab，不为每个任务新开对话，工具按需要出现。

**记忆是自传，不是数据库。**

| 层 | 是什么 | 性质 |
|---|---|---|
| Scene | 你说过的原话、时间、情境 | 不可变，是唯一事实来源 |
| Dreaming | 后台跨对话合成 | 产生假设，带证据与反证 |
| Portrait | 写成人能读的散文 | 版本化，可回滚 |
| Index | 人物/主题/项目/决策/资源/时间 | 只用于检索，不进提示词 |

合成只看原话，从不基于上一版人物志再合成，避免错误一轮轮放大。
时间会改写：「七月要搬去新加坡」三个月后变成「2026 年 7 月搬去了新加坡」，旧事实标记为过期而不是被删掉——所以你能看到「过去的我」和「现在的我」。

**不静默降级。** 这一轮注入了什么、检索到什么、用了多少 token、命中多少缓存、花了多少钱，全部可见。

## 现在能做什么

- 网页里对话，流式输出，可中断
- 模型自主调用工具，网页显示命令卡片、输出与退出码
- DeepSeek 1M 上下文已写进模型目录（不再是 272k 的兜底值）
- 缓存命中可见（实测 18,944 / 19,356 tokens，约 97.9%）

**还不能：** Dreaming、记忆注入、Memory Summary、项目与场景视图、PDF 管线、上下文检查器。

## 架构

```
浏览器
  → pa-web   (127.0.0.1:15723)  本地 axum 服务，转发事件流
    → pa                          agent 循环：工具、审批、补丁、上下文
      → pa-proxy (127.0.0.1:15722) Responses ↔ Chat Completions 协议桥
        → DeepSeek               默认 deepseek-flash，可替换
```

- **引擎**从 [Codex](https://github.com/openai/codex) 源码构建（Apache-2.0），保留它打磨过的 agent 循环、审批、沙箱、补丁与上下文管理。
- **协议桥**从 [cc-switch](https://github.com/farion1231/cc-switch)（MIT）抽出最小可用部分，只做翻译，不运行完整的第三方应用。
- **界面**是本地 Web 服务而不是 Electron：启动快、内存小、不需要安装。从任何目录启动，项目文件夹在页面里选。

## 快速开始（Windows）

完整构建需要 Rust 和一份 Codex 源码。仓库里不放二进制。

```powershell
# 0. 前置：Rust 工具链、PowerShell 7、DeepSeek API Key

# 1. 取 Codex 源码（放到 research/ 下，已被 gitignore）
git clone https://github.com/openai/codex research/codex

# 2. 写入 DeepSeek 的模型元数据（1M 上下文、30 万压缩阈值）
python -I patches/add-deepseek-model.py research/codex/codex-rs

# 3. 构建引擎。必须用短路径，否则 Windows 会在 v8 依赖上报路径过长
$env:CARGO_HOME = "C:\cg"
$env:CARGO_TARGET_DIR = "C:\pt"
$env:CARGO_NET_GIT_FETCH_WITH_CLI = "true"
$env:GIT_CONFIG_COUNT = "1"; $env:GIT_CONFIG_KEY_0 = "core.longpaths"; $env:GIT_CONFIG_VALUE_0 = "true"
cargo build -p codex-cli --bin codex --manifest-path research/codex/codex-rs/Cargo.toml
New-Item -ItemType Directory -Force bin | Out-Null
Copy-Item "C:\pt\debug\codex.exe" "bin\pa.exe"

# 4. 配置密钥（.env 已被 gitignore）
Copy-Item .env.example .env
#   然后把 DEEPSEEK_API_KEY 填进 .env

# 5. 构建两个本地服务
cargo build --manifest-path pa-proxy/Cargo.toml
cargo build --manifest-path pa-web/Cargo.toml

# 6. 启动
powershell -File start-pa.ps1
```

打开 `http://127.0.0.1:15723`。

> `.pa/config.toml` 是隔离的运行时配置：`CODEX_HOME` 指向项目内，不碰你已有的 Codex / Claude Code / cc-switch 安装与记录。

## 目录

```
docs/          需求基线与调研报告
pa-proxy/      Responses ↔ Chat Completions 协议桥
pa-web/        本地 axum 服务 + 单页界面
patches/       对上游 Codex 的可重放补丁
start-pa.ps1   Windows 一键启动
.pa/           隔离的运行时配置（gitignore）
```

## 路线图

**P0**
- 记忆内核：Scene / Portrait / Index 的 SQLite 结构，后台 Dreaming，Memory Summary
- 注入与检查器：相关记忆自然注入，同时把「注入了什么」显示出来
- 学习管线：PDF 页文本 + 页面图 + 页码引用，错题与复习

**P1**
- 组织视图：项目是关系不是文件夹；时间线与检索
- 迁移：导入 ChatGPT 历史对话；发布构建与安装包

## 明确不做

- **不做跨工具记忆共享**：不碰你已有的 Claude Code、Codex、ChatGPT 的记录，也不让它们读我们的
- **不做 Learning AI**：学习是第一个深垂直场景，不是产品身份
- **不做 3D 实验室**：实验演示交给公开资源，我们负责你的错题和薄弱点
- **不做「把历史全塞进窗口」**：能装下不等于记得住

## 文档

- [需求基线](docs/01-requirements.md)：96 条需求，64 条 P0
- [源码调研](docs/03-research.md)：Codex / DSH 拆解与复用边界

## 许可证

[Apache-2.0](LICENSE)，免费商用。第三方组件与许可见 [NOTICE](NOTICE)。
