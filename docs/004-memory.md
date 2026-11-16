# 004 Memory

记忆是一个**给 agent 用的工具**，不是窗口的后台缓存。它会主动搜，窗口不会替它预取。
这条决定了下面全部：没有 embedding、没有隐式注入、没有"相关记忆"面板。

**记忆只服务 idle。** busy 状态下记忆工具根本不存在（不是"禁止写"，是整个面不可用），所以任务细节不可能渗进来。要整理它们，等回到 idle，从任务卡与 transcript 取料。见 006。

## 存什么

```
~/.odiva/memory/
├── journal/2026-03-15.md    流水：当天发生了什么，只追加
├── facts/                   蒸馏：稳定的、可被推翻的事实
│   ├── project-odiva.md
│   └── preference-editor.md
├── tasks/<id>.md            每个任务一张卡：brief、回报、产物
└── index/                   tantivy 索引，随时可以删了重建
```

- **只有 `journal/` 和 `facts/` 是不可重建的。** `index/` 随时删——这条让索引格式可以随便改。
- `facts/` 每条带 front-matter：`id`、`tags`、`valid_from`、可选的 `superseded_by`。事实会过期（"用户在 A 公司"），过期不是删除，是接一条新的并指回旧的。
- 一律 markdown。可读、可 diff、可手工改、可以进 git。

## 谁写

**daemon 是唯一写者。** agent 只上报"事件"，落盘由 core 决定：什么时候进 journal、什么时候该蒸馏、哪条事实被取代。

写入是 append-only 的 journal，蒸馏是周期性（或按量触发）的单独步骤，两者分开——所以蒸馏失败不会丢流水。

两个水位线回答"整理到哪了"：`consolidated_through`（已蒸馏到哪条消息）与 `indexed_through`（索引追到哪个文件）。它们让整理可续、可去重、可观测。

## 怎么读

六个工具，接口先定死，后端可换：

```rust
memory.search(query, k, since, path_glob) -> [Hit]
memory.read(path, lines?)                 -> String
memory.list(prefix, depth)                -> [Entry]   // 标题 + 日期，不是正文
memory.append(path, content)              -> ()
memory.update(path, content)              -> ()
memory.delete(path)                       -> ()
```

**不用 embedding。** 个人规模（几千篇）下，BM25 加好的 md 结构就够；真正的失败模式不是"语义没算出来"，是**agent 不知道该搜什么词**（用户说"上周那个 HDR 的事"，原话里是别的词）。两个廉价补丁，都比向量检索便宜：

- `memory.list` 让它能**浏览**，而不是靠猜关键词；
- 每次会话开始注入一份 ~2K token 的**近期索引**（最近 N 篇的标题 + 日期 + 路径）。这是 embedding 的替代品：给 agent 一张地图，而不是给它相似度。

## tantivy 的具体坑

1. **中文分词。** 默认 tokenizer 按空格切，中文整段会变成一个 term，BM25 直接失效。必须换 jieba / lindera 一类，并且要处理中英混排 + 代码标识符。
2. **字段加权。** `title` / `tags` / `body` / `date` 分别建字段，标题命中要比正文命中重要。
3. **索引增量维护，`rg` 兜底。** 写入进脏队列，core 按批 commit；查询先查索引，脏区间用 `rg` 扫。索引仍然可以整目录删掉重建——这条让索引格式可以随便改。

## 和 executor 的关系

**记忆不进 executor。** worker 只收一份自包含的 brief；harness 的工具集由用户配置（项目里的 AGENTS.md、skills、它自己的 MCP），我们不再往里注入额外规则。"我们替它决定它需要什么记忆"正是要避免的那种多余的东西。

worker 缺上下文就在报告里说，中间人下一轮补进新 brief。

于是"互相通信"的两个方向仍然齐：**ACP 向上（事件），brief 向下（指令）**，不需要自己发明协议。
