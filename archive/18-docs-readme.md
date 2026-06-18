# Cursor transcript

---

## 1. User

## 文档整理 — 新会话

工作目录: `/root/code/database`

请先阅读 (不要跳过):
1. `AiKv-Workflow/backup/design.md` — 总规程
2. `AiKv-Workflow/backup/PROGRESS.md` — 当前进度
3. `AiKv-Workflow/backup/INDEX.md` — 本章索引 (若已有行则更新)

---

### 本次任务

| 项 | 值 |
|----|-----|
| PROGRESS 总步 | 18 (阶段 2a 第 6 篇, **aidb 汇总最后一篇**) |
| 目标文件 | `aidb/docs/README.md` |
| 本次子步 | 从步 0 开始 |
| 对比深度 | **模块级** (汇总文档; 纯导航链接, 不加深章逐段) |

### 必须遵守

- **确认门控**: 步 0→1→2→3→4, 每步先讨论, 我确认后再下一步
- **步 4**: 先出 **正文草稿**, 我确认后再写入 `aidb/`、更新 INDEX/PROGRESS
- **汇总文档**: **不必** Module Skill frontmatter
- **过程只进 backup/**: INDEX; aidb 不写迁移历史
- **旧文档顺序**: `backup/aidb/` → `aidb-oldmain` 根目录/`docs/` 类 → WiQunTools 查漏 → `wiqun-db`
- **待核实**: 根文档至多简要提及; 详情仍引用 `aidb/ISSUES.md` (不展开排查过程)
- **步 4 落盘前**: 对照 design.md 验收项 — 链到现有根文档与 5 篇 modules 均有效; 与 [README.md](aidb/README.md) 分工不重复长文

### 上下文 (上一篇已完成)

- 步 17 ✅: [`aidb/CONTRIBUTING.md`](aidb/CONTRIBUTING.md) + [`aidb/CHANGELOG.md`](aidb/CHANGELOG.md)
- 步 13–16 ✅: ARCHITECTURE / DESIGN / DEPLOYMENT / README
- 阶段 1 modules 5 篇全部 ✅ (`docs/modules/*.md`)
- 当前 `aidb/docs/` **仅有** `modules/` — **尚无** `docs/README.md`
- `docs/development.md` **尚未创建** (design 目标结构有占位; 本步步 0 可讨论是否仅链占位/跳过)
- 子目录说明 (`examples/README.md`, `tests/README.md`, `.github/README.md`) — design 称 **不在主文档体系**; 导航页是否一行链过去, 步 0 定
- 本会话归档 (可选参考): [`aidb/archive/17-contributing-changelog.md`](aidb/archive/17-contributing-changelog.md)

### 边界提示 (步 0 可细化)

**docs/README.md** — 回答「开发文档从哪读、链到哪」:

- 宜含: **纯链接** 导航 — 根文档表 (ARCHITECTURE/DESIGN/DEPLOYMENT/CONTRIBUTING/CHANGELOG/AGENTS/ISSUES); `docs/modules/` 五篇 (WHEN 一句话 + 链); 可选 `development.md` 占位链
- 不重复: README 的 30 秒入门/feature 表; CONTRIBUTING 测试矩阵; module 正文; ARCHITECTURE 架构图
- 旧稿: backup 侧 **无** 独立 `docs/README.md` — 可能需从根 README 文档表 + oldmain `docs/` 查漏

**与相邻文档分工**

| 文档 | 回答 |
|------|------|
| [README.md](aidb/README.md) | 项目入口; 已有 §文档 短表 |
| **docs/README.md (本篇)** | `docs/` 域导航 hub; modules 索引 |
| 根 ARCHITECTURE/DESIGN/… | 汇总正文 |
| `docs/modules/*.md` | 域级 Skill 正文 |

### 本次不要

- 不要跳过未确认的步骤
- 不要一步写 `docs/development.md` 正文 (除非步 0 你我确认本步顺带占位且我确认)
- 不要开始阶段 2b (aikv 步 19+)
- 不要修改 `design.md` / 流程, 除非我明确要求
- 不要 commit, 除非我明确要求

---

请从步 0 开始, 给出本步交付物, 等我确认。

---

## 2. Assistant

先阅读规程、进度与索引，并查看 `aidb/docs/` 现状，以便给出步 0 交付物。

---

## 3. Assistant

继续查看相邻文档与旧稿来源，以便界定步 0 范围。

---

## 4. Assistant

## 步 0 — 定范围

**PROGRESS 步 18** · 目标 `aidb/docs/README.md` · **阶段 2a 最后一篇 (aidb 汇总完结)**

---

### 1. 本章定位

| 项 | 内容 |
|----|------|
| **回答的问题** | 「开发文档从哪读、按什么路径跳转?」 — 尤其是 `docs/` 域与五篇 module 的 **WHEN 路由** |
| **文档类型** | 纯链接导航 hub; **无** Module Skill frontmatter; **无** 正文深度 |
| **对比深度** | **模块级** — 旧稿只查漏结构/遗漏链接, 不做逐段对照 |
| **不覆盖** | 源码实现、架构图、设计 rationale、测试矩阵、module 正文、迁移历史 |

**与相邻文档分工** (已对齐 design + 步 16/17 结论):

| 文档 | 职责 | 本篇关系 |
|------|------|----------|
| [`aidb/README.md`](aidb/README.md) | 项目入口; §文档 **扁平短表** (含 5 modules) | 不重复 quick start / feature / bench |
| **本篇 `docs/README.md`** | `docs/` 域 hub; modules **WHEN 一句话** + 根文档分组 | 主交付 |
| 根 ARCHITECTURE / DESIGN / … | 汇总正文 | 只链, 不摘抄 |
| `docs/modules/*.md` | 域级 Skill 正文 | 只链 + WHEN, 不展开 |
| [`CONTRIBUTING.md`](aidb/CONTRIBUTING.md) | hooks / CI / **完整测试矩阵** | 只链 (步 17 选 A) |
| [`ISSUES.md`](aidb/ISSUES.md) | 待核实详情 | 至多一行入口 |

---

### 2. 本章边界 (含 / 不含)

**宜写入 `docs/README.md`:**

1. 开篇 1–2 句: 本文是 `docs/` 导航; 项目入口见 [`../README.md`](../README.md)
2. **根文档表** — ARCHITECTURE, DESIGN, DEPLOYMENT, CONTRIBUTING, CHANGELOG, AGENTS, ISSUES (各一行说明 + 链)
3. **`docs/modules/` 五篇** — 文件名 + **WHEN 一句话** (取自各 module frontmatter `description` 的 Use when 部分, 中文或英中混合均可, 与步 16 约定一致)
4. **可选** [`development.md`](aidb/docs/development.md) — 见下文决策 §4
5. **可选** 子目录 README 一行链 — 见下文决策 §5

**明确不写:**

- README 的 30 秒入门、特性表、示例/bench 表
- CONTRIBUTING 测试矩阵、hooks 细节
- ARCHITECTURE mermaid / 数据流
- module 内代码地图、invariant、任务步骤
- ISSUES 排查过程 (根文档一行链即可)
- 旧 INDEX / oldmain 文档树、迁移对照

**源码关联:** 本篇 **不绑定** 单一 `src/` 路径; 步 1 以 **现有 aidb 文档树 + 链接有效性** 为准 (验收项: design §验收标准).

---

### 3. 拟读文件列表 (步 1–4 用)

**当前 aidb (步 1 主依据 — 链目标必须存在):**

| 类别 | 路径 |
|------|------|
| 邻接入口 | `aidb/README.md`, `aidb/AGENTS.md`, `aidb/CLAUDE.md` |
| 根汇总 (6+2) | `ARCHITECTURE.md`, `DESIGN.md`, `DEPLOYMENT.md`, `CONTRIBUTING.md`, `CHANGELOG.md`, `ISSUES.md` |
| modules (5) | `docs/modules/{engine,engine-storage,cluster,backup,observability}.md` |
| 缺失占位 | `docs/development.md` (**不存在**, 步 4 前决定是否建占位) |

**旧文档 (步 2 查漏, 模块级):**

| 顺序 | 路径 | 预期 |
|------|------|------|
| 1 | `AiKv-Workflow/backup/aidb/` | **无** 独立 `docs/README.md`; 根 README §文档 为扁平表参考 |
| 2 | `aidb-oldmain/INDEX.md`, `README.md` §文档导航 | 结构参考; 内容 **大量过时** (USER_GUIDE、WAL_IMPLEMENTATION 等已废弃) |
| 2 | `aidb-oldmain/docs/DOCUMENT_STRUCTURE.md` | 旧 docs 树说明; 仅查漏 |
| 3 | WiQunTools `wiqun-db-inventory/` | 步 16 已判无入口专章 — **跳过** |
| 4 | `wiqun-db/` | 与 backup 重复 — **跳过** |

**过程参考 (不写进 aidb):**

- [`aidb/archive/16-readme.md`](aidb/archive/16-readme.md) — README §文档 与 docs hub 分工
- [`aidb/archive/17-contributing-changelog.md`](aidb/archive/17-contributing-changelog.md) — development.md 选 A

---

### 4. 步 0 待你拍板的三项

#### 4a. `docs/development.md` (步 17 已选 **方案 A**)

| 选项 | 做法 | 建议 |
|------|------|------|
| **A1 (推荐)** | 本篇 **只写一行**: 「构建/feature/测试矩阵见 CONTRIBUTING + DEPLOYMENT」; **不创建** `development.md` 文件 | ✅ 与步 17、DEPLOYMENT 分工一致 |
| A2 | 步 4 **同时** 创建 `development.md` 占位 (标题 + 「待完善」+ 链 CONTRIBUTING/DEPLOYMENT) | design 目标树有该文件, 但正文另开 |
| A3 | 步 4 写 `development.md` 短导航正文 (仍不搬测试矩阵) | 与 A1 差异小, 多维护一篇 |

**默认建议 A1**; 若你选 A2, 步 4 落盘时一并创建占位文件.

#### 4b. 子目录 README (`examples/`, `tests/`, `.github/`)

design: **不在主文档体系内**.

| 选项 | 做法 | 建议 |
|------|------|------|
| **B1 (推荐)** | 本篇 **不写**; 仍由根 README (examples/bench) + AGENTS/CONTRIBUTING (`.github`) 覆盖 | 导航页更纯 |
| B2 | 文末 **「其它」** 小节各一行: examples / tests / `.github/README.md` | 方便从 docs 域一键跳转 |

**默认建议 B1**.

#### 4c. 根 `README.md` §文档 是否改链

当前 README **直接列** 5 篇 modules, **未** 指向 `docs/README.md`.

| 选项 | 做法 | 建议 |
|------|------|------|
| **C1 (推荐)** | 步 4 顺带: §文档 首行或末行加 **「开发文档 hub → [docs/README.md](docs/README.md)」**; 保留现有扁平表 (或 modules 改为「详见 docs/README §模块」) | 双入口: 入口快跳 + docs 域 hub |
| C2 | **不改** README; 仅新建 `docs/README.md` | 最小 diff; 可能略重复 |

**默认建议 C1 (轻量)** — 仅加 hub 链, 不删现有表 (除非你希望 modules 行收敛到 hub).

---

### 5. 拟 INDEX 行 (确认后写入 `backup/INDEX.md`)

| 新文档章节 | 状态 | 参考的旧文档 | 处理 | 备注 |
|------------|------|--------------|------|------|
| `docs/README.md` (导航) | 🔄 步 0 | `backup/aidb/README.md` §文档; `oldmain/INDEX.md` (结构查漏); WiQunTools/wiqun-db **跳过** | 待步 4 落盘 | 纯链接 hub; modules WHEN; development 按 §4a |

---

### 6. 步 4 验收清单 (design 对齐, 步 0 先锁定)

- [ ] 链到现有 **6 篇根文档** + AGENTS + ISSUES 均有效
- [ ] 链到 **5 篇** `docs/modules/*.md` 均有效
- [ ] 与 [`README.md`](aidb/README.md) **不重复长文** (无 feature 表、无架构图、无测试矩阵)
- [ ] 无 Module Skill frontmatter
- [ ] 过程/旧文档对照 **只进** `backup/INDEX.md`, aidb 无迁移史
- [ ] (若 C1) 根 README 与 hub **互相可发现**
- [ ] 完成后 PROGRESS 步 18 ✅ → **阶段 2a 完结**, 可开 2b (aikv 步 19)

---

### 7. 步 0 交付物小结

| 交付物 | 状态 |
|--------|------|
| 本章边界 (含/不含) | ✅ 上文 §1–§2 |
| 拟读文件列表 | ✅ 上文 §3 |
| INDEX 拟增行 | ✅ 上文 §5 |
| 三项决策 (development / 子目录 README / 根 README) | ⏳ 待你确认 |
| 写入 aidb / 更新 PROGRESS | ❌ 步 4 确认后再做 |

---

请确认或调整:

1. **边界** §1–§2 是否 OK?
2. **`docs/development.md`**: **A1 / A2 / A3**? (默认 **A1**)
3. **子目录 README**: **B1 / B2**? (默认 **B1**)
4. **根 README 改链**: **C1 / C2**? (默认 **C1** 轻量加 hub 链)

回复「可以」或具体修改意见后, 我再进入 **步 1** (梳理当前文档树与链接/WHEN 要点).

---

## 5. User

可以

---

## 6. Assistant

步 0 已确认，开始步 1：梳理当前文档树、链接目标与各 module 的 WHEN 要点。

---

## 7. Assistant

## 步 1 — 读当前文档 (以 aidb 现有文档树为准)

步 0 决策已锁定: **A1** (不建 `development.md`) · **B1** (不写子目录 README) · **C1** (根 README 轻量加 hub 链).

---

### 1. 当前 `aidb/` 文档树

```shell
aidb/
├── README.md              # 项目入口; §文档 扁平表 (尚无 docs/README 链)
├── ARCHITECTURE.md        # 分层、数据流、AiKv 边界
├── DESIGN.md              # 跨模块设计 why; 含 modules 阅读导航表
├── DEPLOYMENT.md          # 构建、feature、嵌入、运维
├── CONTRIBUTING.md        # hooks、CI、测试矩阵、PR
├── CHANGELOG.md           # 版本历史
├── AGENTS.md              # AI 助手; 进一步阅读未链 modules
├── CLAUDE.md              # → AGENTS.md
├── ISSUES.md              # 待核实跟踪
└── docs/
    ├── modules/           # 5 篇 Skill 正文 ✅
    │   ├── engine.md
    │   ├── engine-storage.md
    │   ├── cluster.md
    │   ├── backup.md
    │   └── observability.md
    ├── README.md          # ❌ 待建 (本篇)
    └── development.md     # ❌ 不存在 (A1: 本篇一行替代)
```

**链接核验 (步 4 目标):** 根文档 7 篇 + modules 5 篇 + ISSUES **全部存在**; `docs/development.md` **不存在** (符合 A1).

---

### 2. 根文档 — 一行说明 (拟用于 hub 表)

| 文档 | 何时读 (hub 用) | 与 README §文档 关系 |
|------|----------------|---------------------|
| [ARCHITECTURE.md](../ARCHITECTURE.md) | 要总览分层、数据流、与 AiKv 边界 | README 已有, **同义不扩写** |
| [DESIGN.md](../DESIGN.md) | 要理解选型与跨模块 why | 同上 |
| [DEPLOYMENT.md](../DEPLOYMENT.md) | 要构建、feature、嵌入、数据目录与运维 | 同上 |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | 要本地验证、测试矩阵、PR 流程 | 同上 |
| [CHANGELOG.md](../CHANGELOG.md) | 要查版本变更 | 同上 |
| [AGENTS.md](../AGENTS.md) | AI 助手 / CI 速览入口 | README 已有 |
| [ISSUES.md](../ISSUES.md) | 待核实与已知疑点 (详情在此, hub 不展开) | README 已有 |

**注意:** `DESIGN.md` 内已有 **「阅读导航 → modules」** 表 — 本篇 modules 表与之 **并列**, hub 侧重 **WHEN 路由**, DESIGN 侧重 **why 后深入哪篇**; 不互相摘抄长段.

---

### 3. `docs/modules/` — WHEN 一句话 (拟 hub 表)

取自各篇 frontmatter `description` 的 Use when 部分, 压缩为 hub 一行:

| Module | WHEN (hub 草稿) |
|--------|-----------------|
| [engine.md](modules/engine.md) | 改 `engine/{wal,memtable,db}`、写路径 / WAL 恢复 / MemTable flush / Snapshot |
| [engine-storage.md](modules/engine-storage.md) | 改 SSTable / compaction / Bloom / cache / checkpoint; flush 或读放大 |
| [cluster.md](modules/cluster.md) | 改 `cluster/*`、MetaRaft / Multi-Raft / slot 路由 / 迁移; 需 `cluster` feature |
| [backup.md](modules/backup.md) | 改 `backup/*`、BackupManager / 恢复 / 保留策略 |
| [observability.md](modules/observability.md) | 改 `metrics.rs` / cluster metrics、嵌入方注册 `aidb_*`; 需 `monitoring` feature |

**依赖顺序 (hub 可选 1 句提示, 非正文):** engine → engine-storage → (cluster | backup | observability 相对独立).

---

### 4. 阅读路径 (hub 开篇可 3  bullet, 不写长文)

| 场景 | 路径 |
|------|------|
| **首次了解** | [`../README.md`](../README.md) → ARCHITECTURE → 按需 modules |
| **改某域代码** | 下表 modules WHEN → 对应 module; 跨域边界见 module 内「不覆盖」 |
| **构建 / 测试 / PR** | DEPLOYMENT + CONTRIBUTING (+ AGENTS 速览) |
| **AI 助手** | AGENTS → 本 hub modules 表 → 单篇 module |

**构建 / feature / 测试 (A1):** hub **单独一行** — 「构建、feature 与完整测试矩阵见 [DEPLOYMENT.md](../DEPLOYMENT.md) 与 [CONTRIBUTING.md](../CONTRIBUTING.md)」; **不** 创建 `development.md`.

---

### 5. 与邻文档重复度检查

| 内容 | README | DESIGN | 本篇 hub |
|------|--------|--------|----------|
| 特性 / quick start | ✅ 有 | ❌ | ❌ 不写 |
| 根文档短表 | ✅ 扁平 11 行 | ❌ | ✅ 分组 (汇总 vs modules) |
| modules 短描述 | ✅ 无 WHEN | ✅ 域→链 | ✅ **+ WHEN** |
| 架构 mermaid | ❌ | ARCHITECTURE | ❌ |
| 测试矩阵 | ❌ | ❌ | ❌ (链 CONTRIBUTING) |
| ISSUES 详情 | 一行 |  scattered 引用 | 一行链 |

**C1 拟改 README:** §文档 表首或表前加一行: 「开发文档 hub → [docs/README.md](docs/README.md)」; **保留** 现有扁平表 (不删 modules 行).

---

### 6. 步 1 结论 / 步 2 输入

- 本篇 **无源码地图**; 步 2 旧文档查漏以 **导航结构** 为主 (backup README §文档、oldmain INDEX), 非实现对照.
- `AGENTS.md` §进一步阅读 **未** 链 `docs/` — **不在本篇范围** (除非你步 4 前要求顺带补).
- `CLAUDE.md` 仅指向 AGENTS — hub **不必** 单独列 (可选省略).
- 验收: 链 12 个目标有效; 篇幅预估 **~40–55 行** 纯 Markdown 表 + 短引言.

---

### 7. 步 1 交付物

| 交付物 | 状态 |
|--------|------|
| 文档树与链接存在性 | ✅ §1 |
| 根文档 hub 一行说明 | ✅ §2 |
| modules WHEN 草稿 | ✅ §3 |
| 阅读路径 + A1 替代 development | ✅ §4 |
| 与 README/DESIGN 去重边界 | ✅ §5 |
| 写入 aidb | ❌ 步 4 |

---

请确认步 1 后进入 **步 2** (查旧文档: backup/aidb、oldmain INDEX、WiQunTools 查漏). 回复「可以」或调整意见即可.

---

## 8. User

可以

---

## 9. Assistant

步 1 已确认，开始步 2：按旧文档顺序查漏导航结构。

---

## 10. Assistant

## 步 2 — 查旧文档 (导航结构查漏)

按 design 顺序查阅; 本篇为 **纯链接 hub**, 旧稿几乎无同名文件, 结论以 **结构/分组/遗漏链** 为主.

---

### 1. 旧文档清单与分类

| 序 | 路径 | 分类 | 处理建议 |
|----|------|------|----------|
| 1a | `AiKv-Workflow/backup/aidb/README.md` §设计文档 | **部分仍有效** | 仅 3 链: ARCHITECTURE / DESIGN / DEPLOYMENT; **无** `docs/` hub、**无** modules — 步 13–17 已扩展, 本篇补全 |
| 1b | `backup/aidb/docs/` | **无有效内容 — 跳过** | 仅有 observability 旧稿 + superpowers; **无** `docs/README.md` |
| 2a | `aidb-oldmain/INDEX.md` | **已过时** | 根级 mega 索引 (核心 / Multi-Raft / 开发 / 运维 / 历史); 大量路径已不存在或已拆入 modules |
| 2b | `aidb-oldmain/README.md` §文档导航 | **已过时** | 同 INDEX 思路; 含 USER_GUIDE、DEVELOPMENT、FOOLPROOF_OPS、aidb-admin 等 — 步 15–17 已判跳过或迁入 |
| 2c | `aidb-oldmain/docs/DOCUMENT_STRUCTURE.md` | **已过时** | 旧树: 根 `INDEX.md` + `docs/{ARCHITECTURE,DEVELOPMENT,...}`; **无** `docs/README.md` 概念 |
| 3 | WiQunTools `wiqun-db-inventory/` | **无有效内容 — 跳过** | 模块 inventory, 无项目文档导航专章 (与步 16 一致) |
| 4 | `wiqun-db/` | **无有效内容 — 跳过** | 无 `docs/README.md`; 与 backup 重叠 |

**结论:** 无旧稿可「回迁」; 本篇需 **从零写**, 参考物仅为 oldmain **分组思路** (入门 → 汇总 → 域文档 → 贡献), 不复制链接列表.

---

### 2. oldmain 导航 vs 新结构 (可借鉴 / 丢弃)

| oldmain 分组 | 代表链 | 新文档归属 | 本篇 hub |
|--------------|--------|------------|----------|
| 快速开始 | README | [`README.md`](../README.md) | 链回根入口, 不重复 |
| 架构 / 设计 | docs/ARCHITECTURE, DESIGN_DECISIONS | 根 ARCHITECTURE / DESIGN | 根文档表 2 行 |
| 开发 | DEVELOPMENT, CICD | CONTRIBUTING + DEPLOYMENT (A1 无 development.md) | 根表 + A1 一行 |
| 实现细节 | WAL_*, SSTABLE_*, … 单篇 | `docs/modules/*` (5 域) | **modules WHEN 表** (步 1 草稿) |
| 运维 | BACKUP_RECOVERY, PERFORMANCE, monitoring | DEPLOYMENT + modules backup/observability | 根 DEPLOYMENT + module 链 |
| 规划 / 历史 | IMPLEMENTATION, archive/, TODO | **丢弃** | 不写 |
| 根 INDEX | 学习路径、按角色 | 简化为步 1 §4 三条 bullet | 不写 oldmain 式长路径 |

**不迁入:** Milestone 表、100% 完成度、aidb-admin、`:9191/metrics`、48 周 IMPLEMENTATION、docs/archive 树.

---

### 3. backup 旧 README §设计文档 (对照)

```100:104:AiKv-Workflow/backup/aidb/README.md
## 设计文档

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [DESIGN.md](DESIGN.md)
- [DEPLOYMENT.md](DEPLOYMENT.md)
```

| 项 | 步 17 后现状 | 本篇需补 |
|----|--------------|----------|
| 根汇总 3 篇 | ✅ 已存在且更完整 | + CONTRIBUTING, CHANGELOG, AGENTS, ISSUES |
| modules | ❌ 旧稿无 | ✅ 5 篇 WHEN 表 |
| 测试/构建 | 旧 README 内嵌测试矩阵 | A1: 链 DEPLOYMENT + CONTRIBUTING, 不嵌表 |
| docs 域入口 | 无 | ✅ 本篇即 hub |

---

### 4. 步 2 消化 / INDEX 处理 (步 4 末执行)

| 旧文档 | 步 4 后处理 |
|--------|-------------|
| `backup/aidb/README.md` §设计文档 | **不删** (backup 归档); INDEX 记「结构参考, 已由 hub  supersede」 |
| oldmain INDEX / DOCUMENT_STRUCTURE | **不删** (只读源); 无对应源文件需删 |
| WiQunTools / wiqun-db | 无动作 |

---

### 5. ISSUES

本篇为导航, 步 2 **无新 ISSUE**. 已有根/module 引用 (如 ISSUE-014) 在 hub **不展开**, 仅保留 ISSUES 一行链 (与步 1 一致).

---

### 6. 拟 INDEX 更新 (步 4 一并写入)

在步 0 拟行基础上补充步 2 结论:

| 新文档章节 | 状态 | 参考的旧文档 | 处理 | 备注 |
|------------|------|--------------|------|------|
| `docs/README.md` (导航) | 🔄 步 2 | `backup/aidb/README.md` §设计文档; `oldmain/INDEX.md` + `DOCUMENT_STRUCTURE.md` (分组查漏); WiQunTools/wiqun-db **跳过** | 待步 4 落盘 | 无旧稿回迁; modules WHEN; A1/B1/C1 |

---

### 7. 步 2 交付物小结

| 交付物 | 状态 |
|--------|------|
| 旧文档清单 + 分类 | ✅ §1 |
| 分组借鉴 vs 丢弃 | ✅ §2 |
| backup 对照 | ✅ §3 |
| 消化策略 | ✅ §4 |
| 新 ISSUES | 无 |
| 写入 aidb | ❌ 步 4 |

**步 3 预期:** 对比深度为模块级; 旧侧 **无** `docs/README.md` 实现, 步 3 仅确认「oldmain 根 INDEX → 新 docs/README + 根 README 双入口」差异表, 无需 oldmain 代码对照.

---

请确认步 2 后进入 **步 3** (模块级差异摘要). 回复「可以」或调整意见即可.

---

## 11. User

可以

---

## 12. Assistant

步 2 已确认。步 3 为模块级对比：旧侧无 `docs/README.md`，重点对照 oldmain 根 INDEX 与新 hub 的结构差异。

---

## 13. Assistant

## 步 3 — 对比旧结构 (模块级)

旧侧 **无** `aidb/docs/README.md` 或同名实现; 对照对象为 **oldmain 根 `INDEX.md` + `docs/` 树**, 以及 aikv-oldmain 的 `docs/index.md` (仅作形态参考, 不迁入 aidb).

---

### 1. 导航形态差异表

| 维度 | oldmain (`INDEX.md` + `docs/*`) | 当前 aidb (步 13–17 后) | 本篇 `docs/README.md` 应如何写 |
|------|----------------------------------|-------------------------|--------------------------------|
| **Hub 位置** | 仓库根 `INDEX.md` (与 README 并列) | 根 `README.md` §文档 扁平表 | **`docs/` 域 hub**; 根 README 加一行链 (C1) |
| **汇总文档** | 多在 `docs/ARCHITECTURE` 等子目录 | 根目录 ARCHITECTURE / DESIGN / DEPLOYMENT / … | 相对链 `../*.md`; **分组表**, 不嵌正文 |
| **实现细节** | 多篇单模块 (`WAL_IMPLEMENTATION`, …) | 5 篇 `docs/modules/*.md` (Skill) | **modules WHEN 表**; 不链旧单篇 |
| **开发/测试** | `docs/DEVELOPMENT.md`, `CICD.md` | `CONTRIBUTING.md` (+ DEPLOYMENT 构建) | A1: 一行链两篇; **不建** `development.md` |
| **运维** | FOOLPROOF_OPS, monitoring/, admin | `DEPLOYMENT.md` + modules backup/observability | hub 链 DEPLOYMENT; 细节进 module |
| **规划/历史** | IMPLEMENTATION, archive/, TODO | **已废弃** | **不写** |
| **体量** | INDEX ~280 行, 多 emoji/学习路径 | README ~89 行 | 目标 **~40–55 行**, 纯表 + 短引言 |
| **AI 路由** | 无 | modules frontmatter + AGENTS | hub 提供 **WHEN 一行**; 不重复 module 正文 |

---

### 2. 路径映射 (oldmain → 新文档, hub 不写对照表)

| oldmain 典型链 | 新归属 | hub 动作 |
|----------------|--------|----------|
| `INDEX.md` / README §文档导航 | `README.md` + **`docs/README.md`** | 双入口; hub 不复制 INDEX |
| `docs/ARCHITECTURE.md` | `ARCHITECTURE.md` (根) | 根文档表 1 行 |
| `docs/DESIGN_DECISIONS.md` | `DESIGN.md` | 根文档表 1 行 |
| `docs/DEVELOPMENT.md`, `CICD.md` | `CONTRIBUTING.md` | 根表 + A1 构建/测试行 |
| `docs/USER_GUIDE.md` | `README.md` quick start + examples | hub 链回 README |
| `docs/*_IMPLEMENTATION.md` | `docs/modules/engine*.md` | modules 表 |
| `docs/BACKUP_RECOVERY`, monitoring | `DEPLOYMENT` + `modules/backup`, `observability` | 根 + module 链 |
| `docs/IMPLEMENTATION`, `archive/` | — | **丢弃** |

**文档表述建议:** 开篇说明「汇总在仓库根目录, 域实现见 `modules/`」, **不** 提 oldmain 路径或迁移.

---

### 3. 与 aikv-oldmain `docs/index.md` 的形态对比 (仅参考)

| 项 | aikv-oldmain `docs/index.md` | aidb 本篇 (已定) |
|----|------------------------------|------------------|
| 位置 | `docs/index.md` | `docs/README.md` (design 约定) |
| 内容 | tree + 按角色学习路径 + 文档统计 | **无 tree**; 根表 + modules WHEN + 3 条阅读路径 |
| 深度链 | 链到 guide/development 子树 | 只链 **现有 12 个** 目标 + ISSUES |
| 步 24 | aikv 可类似写 `docs/README.md` | 本篇为 2a 模板, 保持更短 |

---

### 4. 步 3 结论 (文档应如何表述)

1. **角色:** `docs/` 的 **开发文档入口** — 不是第二份 README, 不是 oldmain 式 INDEX.
2. **结构 (拟):**
   - `# AiDb 开发文档` + 1–2 句 (入口见 `../README.md`)
   - `## 阅读路径` — 3 bullet (步 1 §4)
   - `## 汇总文档` — 7 行表 (步 1 §2)
   - `## 按域阅读 (modules)` — 5 行 WHEN 表 (步 1 §3)
   - `## 构建与测试` — A1 一行 (DEPLOYMENT + CONTRIBUTING)
   - `## 待核实` — 一行链 ISSUES (不展开 ISSUE-014)
3. **不写:** mermaid、feature 表、测试矩阵、module 摘录、子目录 README (B1)、`development.md` 文件 (A1).
4. **C1 (步 4 顺带):** `README.md` §文档 表前加 hub 链 — 与 hub 首段互指.
5. **ISSUES:** 无新条目; 步 3 无代码/行为差异.

---

### 5. 拟 INDEX 更新 (步 4 写入)

| 新文档章节 | 状态 | 参考的旧文档 | 处理 | 备注 |
|------------|------|--------------|------|------|
| `docs/README.md` (导航) | 🔄 步 3 | oldmain `INDEX.md` (形态); backup README §设计文档 (3 链); aikv-oldmain `docs/index.md` (仅参考); WiQunTools/wiqun-db 跳过 | 待步 4 落盘 | 模块级; oldmain 根 INDEX → docs hub + 根 README 双入口; A1/B1/C1 |

---

### 6. 步 3 交付物小结

| 交付物 | 状态 |
|--------|------|
| 模块级差异表 | ✅ §1 |
| oldmain 路径映射 (内部用, 不进 aidb) | ✅ §2 |
| 文档表述建议 | ✅ §4 |
| 新 ISSUES | 无 |
| 写入 aidb | ❌ 步 4 草稿待确认 |

---

请确认步 3 后, 我进入 **步 4 正文草稿** (仍不落盘). 回复「可以」或调整 §4 结构意见即可.

---

## 14. User

可以

---

## 15. Assistant

## 步 4 — 正文草稿 (待确认, 未落盘)

以下为 **`aidb/docs/README.md` 全文草稿** + **C1 对 `README.md` 的拟改一行**. 确认后再写入仓库并更新 INDEX / PROGRESS.

---

### 草稿: `aidb/docs/README.md`

```markdown
# AiDb 开发文档

`docs/` 域的开发文档导航. 项目介绍与快速开始见 [README.md](../README.md).

## 阅读路径

- **首次了解** — [README.md](../README.md) → [ARCHITECTURE.md](../ARCHITECTURE.md) → 按需打开下方 modules
- **改某域代码** — 查 [按域阅读](#按域阅读-modules) WHEN → 对应 module; 跨域边界见 module 内「不覆盖」
- **构建 / 测试 / PR** — [DEPLOYMENT.md](../DEPLOYMENT.md) + [CONTRIBUTING.md](../CONTRIBUTING.md); AI 助手速览见 [AGENTS.md](../AGENTS.md)

## 汇总文档

| 文档 | 内容 |
|------|------|
| [ARCHITECTURE.md](../ARCHITECTURE.md) | 分层、数据流、与 AiKv 边界 |
| [DESIGN.md](../DESIGN.md) | 跨模块设计决策 (why) |
| [DEPLOYMENT.md](../DEPLOYMENT.md) | 构建、feature、嵌入、数据目录与运维 |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | hooks、CI、测试矩阵、提交/PR 规范 |
| [CHANGELOG.md](../CHANGELOG.md) | 版本变更记录 |
| [AGENTS.md](../AGENTS.md) | AI 助手与 CI 入口 |
| [ISSUES.md](../ISSUES.md) | 待核实与已知疑点 |

## 按域阅读 (modules)

| Module | 何时读 |
|--------|--------|
| [engine.md](modules/engine.md) | 改 `engine/{wal,memtable,db}`; 写路径、WAL 恢复、MemTable flush、Snapshot |
| [engine-storage.md](modules/engine-storage.md) | 改 SSTable / compaction / Bloom / cache / checkpoint; flush 或读放大 |
| [cluster.md](modules/cluster.md) | 改 `cluster/*`; MetaRaft / Multi-Raft / slot 路由 / 迁移 (`cluster` feature) |
| [backup.md](modules/backup.md) | 改 `backup/*`; BackupManager、恢复、保留策略 |
| [observability.md](modules/observability.md) | 改 `metrics.rs` / cluster metrics; 嵌入方注册 `aidb_*` (`monitoring` feature) |

依赖顺序: engine → engine-storage; cluster / backup / observability 相对独立.

## 构建与测试

构建、Cargo feature 与完整测试矩阵见 [DEPLOYMENT.md](../DEPLOYMENT.md) 与 [CONTRIBUTING.md](../CONTRIBUTING.md).

## 待核实

详情见 [ISSUES.md](../ISSUES.md) (module 内一行引用, 不在此展开).
```

**篇幅:** 约 45 行 · 无 frontmatter · 12 个链目标 + ISSUES.

---

### 草稿: C1 — `aidb/README.md` §文档 (表前加 1 行)

```markdown
## 文档

开发文档 hub: [docs/README.md](docs/README.md) (汇总文档 + modules WHEN 路由).

| 文档 | 内容 |
|------|------|
| ... (现有表不变) ...
```

---

### 步 4 落盘清单 (确认后执行)

| 动作 | 路径 |
|------|------|
| 新建 | `aidb/docs/README.md` |
| 修改 | `aidb/README.md` §文档 (C1 一行) |
| 更新 | `AiKv-Workflow/backup/INDEX.md` 步 18 行 ✅ |
| 更新 | `AiKv-Workflow/backup/PROGRESS.md` 步 18 ✅; 阶段 2a 完结 |
| **不建** | `docs/development.md` (A1) |
| **不改** | AGENTS / 子目录 README (B1) |
| **不 commit** | 除非你另行要求 |

---

### 验收自检 (design)

- [x] 链到 6 篇根汇总 + AGENTS + ISSUES + 5 modules — 目标均存在
- [x] 不重复 README feature/quick start、CONTRIBUTING 矩阵、ARCHITECTURE 图、module 正文
- [x] 无 Module Skill frontmatter
- [x] 无迁移史 / oldmain 路径

---

请确认草稿 (可回复「可以」或指出修改). 确认后我落盘并更新 INDEX / PROGRESS.

---

## 16. User

可以

---

## 17. Assistant

正在落盘草稿并更新 INDEX/PROGRESS。

---

## 18. Assistant

## 步 4 已完成 — 阶段 2a 完结

### 已落盘

| 文件 | 动作 |
|------|------|
| [`aidb/docs/README.md`](aidb/docs/README.md) | **新建** — 开发文档 hub (~45 行) |
| [`aidb/README.md`](aidb/README.md) | §文档 增加 hub 链 (C1) |
| [`AiKv-Workflow/backup/INDEX.md`](AiKv-Workflow/backup/INDEX.md) | 步 18 行 ✅ |
| [`AiKv-Workflow/backup/PROGRESS.md`](AiKv-Workflow/backup/PROGRESS.md) | 步 18 ✅; **阶段 2a 完结**; Step 0 aidb 勾齐 |

### 未做 (按约定)

- 未建 `docs/development.md` (A1)
- 未改 AGENTS / 子目录 README (B1)
- 未 commit

### aidb 文档体系现状

```shell
aidb/
├── README.md              # 项目入口 + hub 链
├── ARCHITECTURE.md …      # 6 篇根汇总 + AGENTS + ISSUES
└── docs/
    ├── README.md          # ✅ 本篇 (导航 hub)
    └── modules/           # 5 篇 Skill 正文
```

**PROGRESS:** 步 1–18 全部 ✅. 下一步为 **阶段 2b 步 19** — `aikv/ARCHITECTURE.md`. 开新会话时从步 0 走 aikv 汇总即可.

---

## 19. User

使用 vibe-coding/scripts/archive-cursor-session.sh  帮我保存当前对话到 archive 里面 带 tool 和不带 tool 的都保存
