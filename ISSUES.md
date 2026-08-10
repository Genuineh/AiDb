# AiDb — 待核实与问题跟踪

> 位于 aidb 仓库根目录. module 内 **一行引用** 本文件条目 (见 `AiKv-Workflow/backup/design.md` 模板).

**图例**: 状态 = `open` | `confirmed-bug` | `doc-only` | `closed`

---

## 如何使用

1. 文档整理 **步 2–3** 发现设计偏离、实现疑点、oldmain 行为差异时, 在此新增条目.
2. 在对应 module 的 **「待核实」** 小节写: `见 ISSUES.md#ISSUE-NNN — 一句话`
3. 文档整理 **不阻塞** 于修复; 确认要修的 bug 另开开发任务.
4. 关闭条目时更新状态, 必要时回写 module 删除或改写引用.

整理流程中新增 ISSUES 条目, 须在 **步 2–3 确认门控** 内讨论后再写入.

---

## 条目模板 (复制后填写)

```markdown
### ISSUE-NNN: 标题

- **状态**: open
- **发现于**: PROGRESS 步 N / 章节 `docs/modules/xxx.md`
- **相关 src**: `src/...`
- **旧文档**: `aidb-oldmain/docs/...` (可选)
- **oldmain 代码**: `aidb-oldmain/src/...` (可选)
- **现象**: 当前实现 vs 旧设计/旧代码 的差异
- **影响**: 文档应如何描述 / 是否可能是 bug
- **下一步**: 待核实 | 需写测试 | 需开 issue 修代码
```

---

## 条目列表

<!-- 按 ISSUE-NNN 倒序追加 -->
