# Codex / DSH 源码拆解与可复用架构

| 项目 | 内容 |
|---|---|
| 版本 | v0.1 |
| 日期 | 2026-11-08 |
| 对象 | `openai/codex`（Apache-2.0）、`deepseek-ai/deepseek-harness`（MIT） |
| 方法 | 浅克隆当前快照；阅读架构文档、核心 crate/package、压缩/记忆/审批/提示词模板 |
| 目的 | 为实现设计提供可复用机制与边界 |

---

## 0. 结论先行

1. **Codex 是 Rust 生产级 agent 的工程范本**：约 100 个 crate，`core` 管 agent loop/context/tools/approvals/sandbox/memories，`execpolicy` + `sandboxing` + `windows-sandbox-rs` 是审批/隔离的硬工程。
2. **DSH 是“能力如何组合”的架构范本**：Cordis 一切皆插件；Session 是 append-only 事件日志；System Prompt 由 `PromptSection` + `PromptContext` + `ToolProvider` 三层装配；Compaction 是独立 seam；Agent Preset 决定一个会话的能力集。
3. **我们的核心应该借 DSH 的“日志 + 装配”，借 Codex 的“审批/沙箱 + 两阶段记忆”**，而不是复制任一整套。
4. **DSH 是 TypeScript + Electron 桌面**，与“Rust、无 Electron、CLI 级内存”冲突；只借架构，不借运行时。
5. **Codex 的 `memories` 是任务/程序性记忆**，不是我们需要的叙事记忆；它的 Phase 1/2、证据约束、no-op、渐进披露、git diff 遗忘机制可以借，但**记忆模型要学 ChatGPT Dreaming V3**。
6. **Prompt/Compaction 的 KV cache 影响必须作为一等设计约束**；DSH 文档甚至为每个能力写了 “KV Cache effect”。
7. **个人记忆的真正对标是 ChatGPT Dreaming V3**：后台跨对话合成、时间改写、Memory Summary；但它是原地重写、不可逆，我们要做 append-only + 版本化 + 可回滚的 Dreaming。

---

## 1. Codex 拆解

### 1.1 结构

`codex-rs/` 下约 100 个 crate，和本产品最相关的：

| 区域 | crate / 文件 | 作用 |
|---|---|---|
| Agent loop | `core/src/codex.rs`、`codex_thread.rs`、`thread_manager.rs`、`session/` | turn/step、线程、会话 |
| 上下文 | `core/src/context_manager/`、`context-fragments`、`history` | 历史、token 估算、压缩、上下文片段 |
| 压缩 | `core/src/compact*.rs` | 自动/手动/远程/媒体/保留工具输出等多种压缩 |
| 工具 | `core/src/tools/` | 注册、调度、并行、审批、沙箱、工具搜索 |
| 审批 | `core/src/tools/approvals.rs`、`exec_policy.rs`、`guardian/` | 策略、人类审批、自动审查、网络审批 |
| 沙箱 | `sandboxing/`、`linux-sandbox/`、`windows-sandbox-rs/`、`windows-sandbox-service/`、`bwrap/`、`network-proxy/`、`process-hardening/` | 进程/文件/网络隔离 |
| 记忆 | `memories/read/`、`memories/write/` | 读取注入、引用、Phase 1/2 写入 |
| 技能 | `skills/` | SKILL.md 解析、选择、加载、调用 |
| 提示词 | `prompts/` | compact/permissions/approval/guardian/multi-agent 等模板 |
| 持久化 | `rollout/`、`thread-store/`、`history/`、`state/` | rollout、线程、状态 |

许可证：仓库 Apache-2.0；复用代码需保留 `NOTICE`，并逐个确认 crate 的第三方声明。

### 1.2 Agent loop 与上下文

`ContextManager`（`core/src/context_manager/history.rs`）持有：

- `items: Arc<Vec<ResponseItemEnvelope>>`：追加式历史；
- `token_info`、`last_server_reported_usage_item_count`：服务端 usage 锚点；
- `retained_context`：与模型窗口无关的宿主事实；
- `world_state_baseline`、`reference_context_item`：世界状态/上下文基线；
- `history_version`、`reset_version`、`user_message_revision`：压缩/回滚/用户输入版本；
- `guardian_review_mode`：审查策略随历史快照走。

几个值得借的点：

- **token 估算按内容，不按传输元数据**；
- **服务端 usage 作为锚点**，只在其后估算新增项；
- **历史替换（压缩/回滚）有版本号**，避免缓存/重放错乱；
- **保留宿主事实**与模型窗口分离，压缩后可以重新注入；
- **世界状态基线**用于 diff 和重新注入，避免压缩后丢环境。

### 1.3 压缩（compaction）

`compact.rs` 及其兄弟文件显示：Codex 的压缩不是“一个摘要函数”，而是一条完整流水线：

- 自动/手动/远程/媒体/保留工具输出/模型回退/待处理输入等多种策略；
- `PreCompact` / `PostCompact` hooks；
- 压缩前后写 analytics；
- 压缩摘要通过模型生成，替换历史窗口；
- 支持 token budget、模型回退、图片/音频处理、工具输出保留策略；
- 压缩后重建 world state / context。

核心提示词（`prompts/templates/compact/prompt.md`）非常短：

> You are performing a CONTEXT CHECKPOINT COMPACTION. Create a handoff summary for another LLM that will resume the task. Include: current progress and key decisions, important context/constraints/user preferences, what remains, critical data/examples/references.

摘要前缀（`summary_prefix.md`）告诉下一个模型：这是另一个模型留下的摘要，基于它继续，不要重复工作。

**对我们的启示**：

- 压缩摘要不是“聊天总结”，而是**交接文档**：进度、决策、约束、下一步、关键数据；
- 摘要要有“这是上一段工作的交接”前缀，避免模型把它当用户原话；
- 压缩要有 hook、版本、可回滚、可审计；
- 压缩后要重新注入世界状态/项目状态。

### 1.4 审批与沙箱

`ApprovalAction` 枚举覆盖：

- `ExecCommand`：命令、cwd、沙箱权限、额外权限、理由、TTY；
- `WriteStdin`：向运行中的进程写输入；
- `ApplyPatch`：文件、patch、变更；
- `McpToolCall`：MCP server/tool/参数；
- `NetworkAccess`：目标、host、协议、端口；
- `RequestPermissions`：权限请求。

`ApprovalContext` 携带：exec 策略要求、guardian 审查上下文、取消 token、call id、tool name、严格自动审查、审批理由、重试理由、网络审批上下文。

`exec_policy.rs` 显示：

- 规则解析（`PolicyParser`）、规则匹配、决策（allow/deny/ask）、修订（amendment）；
- 危险命令识别（`dangerous_command_match_for_platform`）；
- shell wrapper 降级（bash -lc、powershell -Command 等）；
- 禁止前缀建议（`BANNED_PREFIX_SUGGESTIONS`：bash、sh、zsh、cmd、powershell、python、node、rm、sudo、git 等）；
- 网络规则、权限 profile、沙箱权限；
- `AskForApproval` 策略（Never/OnFailure/OnRequest/…）。

**对我们的启示**：

- 审批不是“每步问/全放开”，而是**按动作类型 + 策略 + 沙箱权限 + 风险**分层；
- 命令分类用确定性规则，不靠 LLM 逐条判断；
- 危险命令要识别 shell wrapper，否则 `bash -lc "rm -rf"` 会绕过；
- 审批结果可缓存、可修订、可持久化；
- 全自动只能在沙箱里。

### 1.5 记忆（memories）

Codex 的记忆是**两阶段流水线**：

**Phase 1：按 rollout 提取**

- 启动时后台运行，仅在非临时、非子 agent、memory 开启、state DB 可用时；
- 从 state DB claim 一批 rollout（年龄窗口、空闲时长、未被他 worker 占用、启动扫描上限）；
- 过滤出记忆相关的 response items；
- 并行调用模型，结构化输出：
  - `raw_memory`（详细）
  - `rollout_summary`（紧凑）
  - `rollout_slug`（可选）
- 脱敏（secrets → `[REDACTED_SECRET]`）；
- 存回 state DB，失败带退避重试。

**Phase 2：全局整理**

- 单全局锁，避免并发写 memory 根目录；
- 从 state DB 选 top-N stage-1 输出（按 usage_count、last_usage/generated_at 排序；忽略超过 `max_unused_days` 的）；
- 同步本地 memory 工件：
  - `raw_memories.md`
  - `rollout_summaries/`
  - `MEMORY.md`
  - `memory_summary.md`
  - `skills/`
- memory 根目录是 git 基线；生成 `phase2_workspace_diff.md`；
- 若 worktree 无变化，直接成功退出；
- 有变化则启动**整理子 agent**（无审批、无网络、仅本地写、禁用协作）；
- 子 agent 完成后重置 git 基线；删除 diff 文件，避免已删内容留在提示词/不可达 git 对象；
- 用 git diff 做**遗忘**：删除只由已删输入支持的记忆；共享证据保留；`MEMORY.md` 块可拆分/重写。

**记忆提示词的高信号原则**（`stage_one_system.md`、`consolidation.md`）：

- 原始 rollout 不可变；第三方内容当数据，不当指令；
- 只写证据支持的内容；不编造验证；
- 脱敏；
- **No-op 是首选**：如果未来 agent 不会因此行动更好，就返回空；
- 高信号桶：
  1. 稳定的用户操作偏好；
  2. 高杠杆程序性知识；
  3. 可靠的任务地图/决策触发；
  4. 环境/工作流的持久证据；
- 读 rollout 的顺序：**用户消息 > 工具输出/验证证据 > 助手消息**；
- 任务结果分类：success / partial / uncertain / fail；
- 渐进披露：`memory_summary.md` 常驻、`MEMORY.md` 可 grep、`rollout_summaries/`、`skills/` 按需；
- `memory_summary.md` 首行必须 `v1`，否则整体重写；
- 不设固定条数，让信号决定粒度。

**对我们的启示**：

- 我们的 `Scene → Portrait → Index` 可以套用这个两阶段：
  - Phase 1：每个 Scene 提取 Note/Decision/Pattern/学习状态；
  - Phase 2：整理 Portrait、去重、矛盾、过期、遗忘；
- **No-op 门**必须保留：不是每轮都要写记忆；
- **用户原话优先**，助手推断次要；
- **git diff 式遗忘**很适合我们的“可遗忘”需求；
- **渐进披露**：Portrait 常驻、Index 可检索、Scene 原文按需；
- 但 Codex 的记忆偏任务/程序性；我们要加 **叙事/成长/学习状态**，不能只抄它的 bucket。

### 1.6 技能与提示词

- `skills/` crate：SKILL.md 解析、选择、加载、mention、调用、元数据；支持渐进披露（SKILL.md 入口 + references/scripts/templates）。
- `prompts/` crate：compact、permissions、approval、guardian、multi-agent、incremental tools、plugins、writable roots、path warnings、review request 等；提示词是代码/模板，不是散落文本。

**对我们的启示**：

- 学习策略、编码规范、审批说明、压缩模板都应该是**版本化模板**，不是硬编码字符串；
- 技能用 SKILL.md + references 渐进披露，适合“教学策略/项目规范/工具用法”；
- 但技能不是核心；核心是 Session/Prompt/Memory/Tools。

---

## 2. DSH 拆解

### 2.1 结构

- `packages/` 约 60 个包；`apps/` 有 cli、desktop、desktop-host、web、plugin-market-generator。
- Cordis：插件贡献 service、typed events、reversible effects；没有特权核心；卸载即回滚。
- Profile = bundle stack + patch；Bundle = 插件配置行 + mount code。
- 内置 profile：`web`、`headless`、`sdk`、`sdk-minimal`、`acp`。
- 核心包：`core/session`、`core/system-prompt`、`core/tools`、`core/agent`、`core/agent-loop`、`core/scope`、`llm/llm`。

许可证：MIT；桌面是 Electron（我们不用）。

### 2.2 Session：append-only 事件日志

`SessionEventMap` 是唯一的模型历史来源：

- `turn/start`、`turn/end`、`step/start`、`step/end`；
- `user/message`、`developer/message`、`system/message`、`assistant/message`、`assistant/attempt`；
- `tool/call`、`tool/result`；
- `request/header`、`request/context`；
- `session/end-seed`（fork 边界）。

关键原则：

- **模型可见即已记录**（Model-visible means logged）；
- 模型历史由 `deriveMessages()` 从日志派生，不单独存；
- 失败的 assistant attempt 也记录（不伪造历史）；
- 工具调用/结果成对记录；取消时补 `ABORTED_BEFORE_DISPATCH` / `TOOL_OUTCOME_UNKNOWN`；
- 请求 header/context 记录，便于重放/审计；
- 压缩用日志锁 bracket，摘要作为 `user/message` replacement，原始事件保留。

**对我们的启示**：

- 这正是我们“Scene 是真相、原话不可变”的工程实现；
- 我们的 Scene 日志应该 append-only，Portrait/摘要只是派生视图；
- 工具调用、审批、检索、上下文注入都要记录，才能做检查器；
- 分支/续接用 seed/end-seed，不需要导出/导入。

### 2.3 System Prompt 装配

三层：

1. **`PromptSection`**：有序、静态或动态、可 `complete`；按 order + name 排序。
2. **`PromptContext`**：动态、cache-safe；在 retained history 之后作为 user-role snapshot 记录，只在变化或压缩后重新记录。
3. **`ToolProviderResult`**：当前装配的 tool schemas + `knownNames`；只暴露当前 scope 允许的工具。

渲染后作为 `system/message` surface node；`systemPromptUpdate: 'in-history'` 允许把 prompt 变化追加在 cached history 之后，避免打断前缀缓存。

**对我们的启示**：

- 稳定核心 = `PromptSection`；任务模块 = 动态 section；检索/人物志 = `PromptContext` 放尾部；工具 = 按 scope 白名单；
- 不要把动态记忆放开头；放尾部或 in-history，保护 KV cache；
- 每次装配的 prompt 要记录快照，供检查器。

### 2.4 Tools 管道

`ToolDefinition` 包含：

- `name`、`description`、`parameters`（模型可见）；
- `output`（canonical JSON schema + render + presentationMeta）；
- `execute(args, exec)`；
- `projectContent`、`finalizeContent`；
- `timeoutMs`、`isConcurrencySafe`；
- `presentCall`、`presentResult`（UI 展示，纯函数，可重放）。

`ToolRestriction`：按 scope 的 allow/deny；子 agent 保留自己的工具；全局工具可被限制。

执行管道：`tools/pre-execute` → `tools/execute` → `tools/post-execute` → `tool/result`；支持并行、超时、取消、审计。

**对我们的启示**：

- 工具应该有 schema、输出契约、展示投影、超时、并发安全、审批；
- 工具集按任务/场景动态加载（白名单），不相关的不进 prompt；
- UI 展示用纯函数投影，保证重放一致；
- 检查器可以看到每次工具调用/结果。

### 2.5 Compaction seam

- 抽象 `CompactionEngine`：`compactIfNeeded`、`compactNow`、`compactRegion`；
- 日志 bracket：`compaction/start` → `compaction/summary` → 替换 span 为一条 user-role summary → `compaction/end`；
- 原始事件保留；`deriveMessages()` 渲染摘要 + 保留节点；
- 工具结果可单独 prune；图片可 offload；token meter 决定何时压缩；
- 工具配对边界：压缩边不能切断未回答的 tool call；
- KV cache：替换从第一个被 shadow 的 token 起失效；seam 本身不改请求。

**对我们的启示**：

- 章节压缩应该是一个**可审计的 bracket**，不是偷偷改历史；
- 摘要作为“交接文档”插入，原始 Scene 保留；
- 压缩前先 prune 大工具结果/图片，再总结；
- 压缩边要保证工具调用成对；
- 检查器显示“哪些被 shadow、哪些保留、缓存影响”。

### 2.6 Preset / Skill / Capability Seam

- **Agent Preset**：一个会话选择一组 child plugins；定义预加载，编辑影响后续新 agent；活 agent 组合稳定。
- **Skill**：注册表合并多 provider；按需加载 body；`modelInvocable`/`userInvocable` 策略；本地文件、嵌入式、远程都可。
- **Capability Seam**：Service Definition / Provider / Consumer 三角；换一个 provider 可整体替换能力（fs/subprocess/sandbox/llm/browser）。

**对我们的启示**：

- 教学/工程/工作/判断不是“模式”，是**Preset**：每个 Scene 选择一组工具/提示词/技能；
- 学习策略、编码规范、审批说明用 Skill 按需加载；
- LLM、Sandbox、FS、Search、Memory 都应该是 seam，便于替换/测试；
- 不要做插件市场；内置少量 preset + 少量 skill 即可。

---

## 2.5 Dreaming V3（记忆的真正对标）

> 这一节是后补：Codex/DSH 是任务/工程记忆；个人记忆的真正对标是 ChatGPT 的 Dreaming V3。

### 2.5.1 它是什么

- 2026-06-04 上线；Saved Memories（2024）→ Dreaming V0（2025）→ V3（2026）。
- 不是模型，是 ChatGPT 的记忆架构。
- 后台进程读很多过去对话，抽取、合成、更新“记忆状态”，不靠用户说“记住这个”。
- 管线：extraction → offline consolidation（dreaming）→ long-term store → retrieval into context。
- 时间感知：把“7 月要去新加坡”改写成“2026 年 7 月去了新加坡”。
- Memory Summary 页面：查看/添加/编辑/删除；按主题控制；约每小时更新。
- 官方内部指标：事实召回 41.5%→67.9%→82.8%；偏好遵循 31.4%→55.3%→71.3%；随时间保持正确 9.4%→52.2%→75.1%；算力降约 5x。
- 注意：主页面 403，数字为二手/内部评估，未独立复现。

### 2.5.2 真正厉害的地方

1. 跨对话合成，不是单会话摘要。
2. 后台 consolidation，不占用户等待时间。
3. 时间有效性与改写。
4. 用户可见的记忆摘要，可编辑/删除。
5. 成本工程：5x 降本，才敢免费。

### 2.5.3 风险（不能照抄）

- 原地重写：每轮基于上一轮输出，不可逆；Arize/UIUC 论文指出迭代 consolidation 会退化（GPT-5.4 ARC-AGI 100%→54%）。
- 幻觉：会自信地编造没发生过的细节。
- 删除不等于立刻清除：30 天保留窗口。
- 隐私/广告：记忆被用于个性化广告的担忧。

### 2.5.4 对比

| | Codex/Hermes | Dreaming V3 | 我们 |
|---|---|---|---|
| 记忆单位 | MD/rollout/skill | 合成后的用户记忆状态 | Scene + Note + Portrait |
| 范围 | 任务/仓库 | 跨对话/整个人 | 跨对话/跨项目/人 |
| 触发 | 启动/手动 | 后台/离线 | 后台/离线 + 手动 |
| 更新 | 追加/编辑文件 | 原地重写、时间改写 | 追加 + 版本化 + 时间有效 |
| UI | 文件 | Memory Summary | Memory Summary + Inspector |
| 风险 | 文档膨胀/过期 | 不可逆/幻觉 | 证据/可回滚/可遗忘 |

### 2.5.5 我们的 Dreaming 引擎

```
Scene log（append-only，原话）
   ↓ 每轮/每场景
Candidate notes（事实/偏好/目标/误解/决策/人物）
   ↓ 后台 Dreaming（夜间/空闲）
Portrait（散文，版本化）+ Index（结构化）
   ↓ 每轮检索
Context injection（尾部，cache-friendly）
   ↓
Memory Summary（用户可见、可编辑、可遗忘）
```

原则：

- 原话不可变；Dreaming 只生成派生视图。
- Portrait 版本化；每次 consolidation 产生新版本，可回滚。
- 从 Scene 重新合成，不从上一版 Portrait 生成。
- 时间有效性：`valid_from` / `valid_to`；过去的我 / 现在的我。
- 证据与来源：每条记忆可回到原话；AI 推断单独标记。
- 写入闸门 + Diff 确认；低置信进收件箱。
- 遗忘级联；Memory Summary 可见/可改/可删。
- 离线 consolidation：Flash + 缓存 + 批量。

### 2.5.6 验收标准

- 导入物理/数学/接项目对话，跑一次 Dreaming。
- 问“我物理卡在哪 / 乔瑞丰是谁 / 我最近在做什么”，不用提醒能答。
- 时间改写：旧的“准备去”变成“去过”；过时信息不再出现。
- 每条记忆能点回原话；删除后检索不到。
- 不编造；低置信有标记。

### 2.5.7 来源

- [Gigazine: Dreaming V3](https://gigazine.net/gsc_news/en/20260605-chatgpt-memory-dreaming-v3/)
- [Arize: Two labs, two architectures](https://arize.com/blog/two-labs-started-dreaming-and-they-built-two-different-architectures/)
- [Engadget](https://www.engadget.com/2187811/chatgpt-s-memory-is-getting-better-especially-if-you-re-on-the-free-tier/)
- [Yahoo Tech](https://tech.yahoo.com/ai/chatgpt/articles/im-excited-about-chatgpts-memory-upgrade-103058068.html)
- [36Kr](https://36kr.com/p/3843752341309696)
- [Tencent Cloud](https://cloud.tencent.com.cn/developer/article/2683126)

## 3. 对比与取舍

| 维度 | Codex | DSH | 我们应该 |
|---|---|---|---|
| 语言/运行时 | Rust | TypeScript/Cordis | Rust |
| 桌面 | TUI/CLI | Electron | Tauri/原生，无 Electron |
| 会话模型 | 线程/rollout + ContextManager | append-only SessionEvent log | append-only Scene log |
| Prompt | 代码化模板 + sections | PromptSection + PromptContext + ToolProvider | 借 DSH 三层装配 |
| 压缩 | 多策略流水线 + hooks + 模型摘要 | compaction seam + 日志 bracket + summary replacement | 借 DSH bracket + Codex 交接摘要 |
| 审批 | execpolicy + guardian + sandbox 多级 | approval policy + sandbox seam | 借 Codex 动作分类 + 简化分档 |
| 沙箱 | Linux/Windows/bwrap/network proxy | sandbox 可插拔 | 先工作区隔离，后 OS 级 |
| 记忆 | Phase 1/2、证据、no-op、git diff 遗忘 | Session log + context injection | 两阶段 + Scene/Portrait/Index |
| 技能 | SKILL.md Rust crate | skill registry + invocation policy | 简单 SKILL.md 注册表 |
| 组合 | 插件/hook/MCP | Cordis 一切皆插件 + preset | 原生 capability registry + preset |
| 缓存 | 显式 token/usage/压缩影响 | 每个能力写 KV cache effect | 把 cache 当一等约束 |
| 许可 | Apache-2.0 | MIT | Apache-2.0；复用保留 NOTICE |

**不该抄的**：

- DSH 的 Electron 桌面、Cordis 全插件化、插件市场；
- Codex 的 100 crate 全量复杂度、coding-only 的记忆 bucket；
- 任何一家的完整提示词（版权/场景不匹配）。

**该抄的**：

- DSH：Session log、Prompt 三层、Tool 管道、Compaction bracket、Preset/Skill、Capability Seam、KV cache 文档习惯；
- Codex：审批动作分类、execpolicy、沙箱、两阶段记忆、no-op、渐进披露、git diff 遗忘、apply-patch、skills crate。

---

## 4. 推荐架构（Rust core）

```
┌────────────────────────────────────────────┐
│ UI (Tauri/React)                            │
│  Chat · Artifacts · Inspector · Timeline    │
└──────────────────┬─────────────────────────┘
                   │ commands/events
┌──────────────────▼─────────────────────────┐
│ Agent Loop                                  │
│  turn/step · inbox · cancellation           │
└───┬──────────┬──────────┬──────────┬────────┘
    │          │          │          │
┌───▼───┐ ┌────▼────┐ ┌───▼────┐ ┌───▼─────┐
│Session│ │ Prompt  │ │ Tools  │ │ Memory  │
│ log   │ │ assembly│ │registry│ │ engine  │
└───┬───┘ └────┬────┘ └───┬────┘ └───┬─────┘
    │          │          │          │
┌───▼──────────▼──────────▼──────────▼─────┐
│ Capability seams: LLM · Sandbox · FS ·   │
│ Search · Browser · Storage · Secrets     │
└──────────────────────────────────────────┘
```

模块职责：

- **Session**：append-only Scene/Message/Tool/Approval/Context 事件；派生历史；分支/续接。
- **Prompt**：稳定 section + 动态 context（尾部）+ tool schema 白名单 + 变量；记录每次装配。
- **Tools**：schema/output/execute/present；pre/execute/post；审批；并行；超时。
- **Memory**：Scene → Note → Dreaming → Portrait/Index；Phase 1 提取、Phase 2 后台合成；检索；遗忘；导入/导出。
- **Dreaming**：夜间/空闲跨对话合成；时间改写；矛盾/过期处理；Memory Summary；可回滚。
- **Compaction**：日志 bracket + 交接摘要 + 工具结果 prune + cache 影响记录。
- **Sandbox**：工作区隔离、资源限制、网络策略、危险命令识别、快照/撤销。
- **Presets**：教学/工程/工作/判断，内部能力集，不是 UI 模式。
- **Inspector**：prompt/检索/工具/token/cache/cost 全链路可见。

---

## 5. 许可证与风险

- Codex：Apache-2.0；复用代码保留 `NOTICE`；逐个 crate 检查第三方声明。
- DSH：MIT；复用代码保留版权声明；Cordis 是独立项目，需查其许可证。
- **提示词版权**：不要复制 ChatGPT/Claude/Codex 的系统提示词原文；只学结构，自己写。
- **Electron**：DSH 桌面是 Electron，明确不用；Tauri/原生。
- **复杂度风险**：Codex 的 crate 图、DSH 的 Cordis 插件图都过大；我们只取必要机制，避免“架构崇拜”。
- **场景错配**：Codex 记忆偏 coding；我们的学习/成长记忆需要叙事层，不能直接套。

---

## 6. 对需求的映射

| 需求 | 借什么 |
|---|---|
| CTX-01/02/03 长上下文、自动注入、缓存 | DSH Session log + PromptContext + in-history；Codex token/usage 锚点 |
| CTX-04/05 不丢细节、召回策略 | DSH compaction bracket + raw log；Codex 交接摘要 |
| MEM-01..12 记忆 | Dreaming V3 行为 + Codex Phase 1/2 机制；自研 Scene/Dreaming/Portrait/Index |
| MEM-13..18 Dreaming | 跨对话合成/时间改写/Memory Summary；不做原地重写 |
| MEM-06 遗忘 | Codex git diff 遗忘 + 级联删除 |
| CODE-01..08 工程理解/安全补丁/审批 | Codex execpolicy + ApprovalAction + sandbox + apply-patch + skills |
| OBS-01..04 透明 | DSH request/context 日志 + 我们的 Inspector |
| ORG-01..07 组织 | DSH Session/Scene + 我们的关系/视图；不抄 Cordis 插件市场 |
| LRN-01..09 学习 | 自研；借 Skill/Widget/记忆，不借 coding 记忆 bucket |
| COMM-01 许可 | Apache-2.0；Codex 兼容，DSH MIT 兼容 |

---

## 7. 下一步

1. 基于本报告定实现设计：模块、数据模型、接口、目录、里程碑。
2. 明确“复用 vs 自研”清单：哪些 Codex crate 直接用，哪些只借模式。
3. 先做最小垂直切片：Session log + Prompt 装配 + DeepSeek + 记忆注入 + Inspector。
4. 真实用物理/数学/接项目对话测试；边用边调。

---

*本文是调研报告，不是实现设计。所有复用需在实现前核对具体版本与许可证。*
