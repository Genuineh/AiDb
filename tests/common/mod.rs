//! AiDb 共享测试工具
//! @component aidb-test-harness
//!
//! 各模块的集成测试和回归测试可引用此模块的工具函数.
//! 使用时在测试文件的顶部添加:
//!
//! ```ignore
//! mod common;
//! use common::observability::EventCatcher;
//! use common::dataflow::SpanCollector;
//! ```
//!
//! # 数据流验证模式速查
//!
//! | 模式 | 工具 | 验证什么 |
//! |------|------|---------|
//! | A: Span 父子树 | `dataflow::SpanCollector` + `assert_child_order` | 调用顺序和嵌套关系正确 |
//! | B: Trace 关联 | `dataflow::SpanTree::all_same_trace()` | 跨模块 span 属于同一 trace |
//! | C: Event 时序链 | `observability::EventCatcher::drain()` | 事件发出的先后顺序 |
//! | D: Metrics 联动 | `prometheus::Counter::get()` | 指标间因果关系 |
//! | E: 端到端生命周期 | 纯功能断言 | 数据经过完整路径后不丢不坏 |

#![allow(dead_code)]

pub mod dataflow;
pub mod observability;

// 按需扩展:
// pub mod mock_db;
// pub mod test_config;
// pub mod temp_dir;
