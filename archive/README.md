# 文档整理会话归档 (只读)

本目录存放 **文档整理过程** 的 Cursor 会话导出, **不是** 现行产品文档.

## 用途

- 回顾某 PROGRESS 步的讨论与确认过程
- 对照 module / 根文档是如何写成的

## 文件命名

| 模式 | 说明 |
|------|------|
| `NN-<topic>.md` | 对话正文 (由 `agent-transcripts/*.jsonl` 转换) |
| `tools/NN-<topic>-with-tools.md` | 同上, 含 tool 调用记录 (可选) |

`NN` 对应 `AiKv-Workflow/backup/PROGRESS.md` 步号 (如 `01-engine`, `02-engine-storage`).

## 注意

- **现行文档** 以 `docs/modules/*.md` 与仓库根目录 README/ARCHITECTURE 等为准
- 归档内可能含过时链接 (如早期 `backup/ISSUES.md` 路径), **请勿** 当作当前规范
- 生成工具: `vibe-coding/scripts/cursor-transcript-to-md.py`
