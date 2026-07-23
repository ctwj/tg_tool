// 网盘能力子模块（feature 047）
// US1 阶段：夸克单一驱动，采用模块级自由函数（health_check）。
// 当 US2/后续引入 UC/百度等多驱动时，再抽象为 PanDriver trait（constitution II 面向接口）。

pub mod credential;
pub mod quark;
