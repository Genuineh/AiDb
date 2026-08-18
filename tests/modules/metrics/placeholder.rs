//! monitoring feature 关闭时的占位测.
//! @component aidb-metrics

#[cfg(not(feature = "monitoring"))]
#[test]
fn monitoring_feature_disabled_placeholder() {}
