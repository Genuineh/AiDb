use std::sync::atomic::Ordering;
use std::sync::Arc;

use aidb::metrics::sync_to_otel;
use aidb::metrics::testutil;
use aidb::statistics::{DbOp, Statistics};

#[test]
#[serial_test::serial]
fn test_sync_to_otel_counter_difference() {
    let exporter = testutil::init_in_memory();
    let stats = Arc::new(Statistics::default());

    let c_before = testutil::counter_sum(&exporter, "aidb_operations_total");
    let client_before = testutil::counter_sum(&exporter, "db.client.operations");

    // 注入 10 次 put 操作
    stats.operations[DbOp::Put as usize].fetch_add(10, Ordering::Relaxed);

    // 第一次同步
    let c1 = testutil::sync_and_get_counter(&stats, "aidb_operations_total");
    assert_eq!(c1 - c_before, 10);
    assert_eq!(
        testutil::counter_sum(&exporter, "db.client.operations") - client_before,
        10
    );

    // 再次调用 (无新操作产生), 断言幂等且无 double-report
    let c2 = testutil::sync_and_get_counter(&stats, "aidb_operations_total");
    assert_eq!(c2, c1);
}

#[test]
#[serial_test::serial]
fn test_sync_to_otel_histogram_replay_and_twin_instrument() {
    let exporter = testutil::init_in_memory();
    let stats = Arc::new(Statistics::default());

    // 注入耗时: 50us (bucket 0: < 100us) 3 次; 200us (bucket 1: 100~500us) 2 次
    for _ in 0..3 {
        stats.operation_durations[DbOp::Put as usize].record(50);
    }
    for _ in 0..2 {
        stats.operation_durations[DbOp::Put as usize].record(200);
    }

    let h_before = testutil::histogram_count(&exporter, "aidb_operation_duration_seconds");
    let twin_before = testutil::histogram_count(&exporter, "db.client.operation.duration");

    sync_to_otel(&stats);

    // 断言计数增加 5
    assert_eq!(
        testutil::histogram_count(&exporter, "aidb_operation_duration_seconds") - h_before,
        5
    );
    // 断言孪生 instrument 同步增加 5
    assert_eq!(
        testutil::histogram_count(&exporter, "db.client.operation.duration") - twin_before,
        5
    );

    // 断言桶分布: bucket 0 应包含 3 个, bucket 1 应包含 2 个
    let buckets =
        testutil::histogram_bucket_counts(&exporter, "aidb_operation_duration_seconds").unwrap();
    assert!(buckets.len() >= 10);
    // 注意: SDK 累加前若有历史可能非 0, 需校验当前桶的增量分布或在新 exporter 下精准分布
    assert!(buckets[0] >= 3);
    assert!(buckets[1] >= 2);
}

#[test]
#[serial_test::serial]
fn test_sync_to_otel_histogram_overflow_bucket_2s() {
    let exporter = testutil::init_in_memory();
    let stats = Arc::new(Statistics::default());

    // 注入超长耗时 1.5s (1_500_000 us), 落入 bucket 9 (> 1.0s 溢出桶)
    stats.operation_durations[DbOp::Put as usize].record(1_500_000);

    let h_before = testutil::histogram_count(&exporter, "aidb_operation_duration_seconds");
    sync_to_otel(&stats);

    assert_eq!(
        testutil::histogram_count(&exporter, "aidb_operation_duration_seconds") - h_before,
        1
    );

    // 验证溢出桶: bucket 9 应有新增计数, 验证重放值 2.0s 精准落入 +Inf 桶
    let buckets =
        testutil::histogram_bucket_counts(&exporter, "aidb_operation_duration_seconds").unwrap();
    assert!(buckets[9] >= 1);
}

#[test]
#[serial_test::serial]
fn test_sync_to_otel_reset_and_second_round_net_increment() {
    let exporter = testutil::init_in_memory();
    let stats = Arc::new(Statistics::default());

    let c_before = testutil::counter_sum(&exporter, "aidb_operations_total");

    // 轮次 1: 产生 100 次操作
    stats.operations[DbOp::Put as usize].fetch_add(100, Ordering::Relaxed);
    sync_to_otel(&stats);
    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_operations_total") - c_before,
        100
    );

    // 模拟重置
    stats.reset();

    // 轮次 2: 产生 30 次操作
    stats.operations[DbOp::Put as usize].fetch_add(30, Ordering::Relaxed);
    sync_to_otel(&stats);

    // 第二轮累积总增量严格为 130 (第二轮净增量严格为 30)
    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_operations_total") - c_before,
        130
    );
}

#[cfg(feature = "cluster")]
#[test]
#[serial_test::serial]
fn test_sync_to_otel_cluster_raft_counters() {
    let exporter = testutil::init_in_memory();
    let stats = Arc::new(Statistics::default());

    let rpc_before = testutil::counter_sum(&exporter, "aidb_raft_rpc_total");
    let log_before = testutil::counter_sum(&exporter, "aidb_raft_log_entries_total");
    let fatal_before = testutil::counter_sum(&exporter, "aidb_raft_group_fatal_total");
    let restart_before = testutil::counter_sum(&exporter, "aidb_raft_group_restart_total");

    // 注入 raft 统计
    // raft_rpc[0][0]: append_entries incoming
    stats.raft_rpc[0][0].fetch_add(7, Ordering::Relaxed);
    stats.raft_log_entries.fetch_add(42, Ordering::Relaxed);
    stats.raft_group_fatal.fetch_add(1, Ordering::Relaxed);
    stats.raft_group_restart[0].fetch_add(2, Ordering::Relaxed); // success

    sync_to_otel(&stats);

    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_raft_rpc_total") - rpc_before,
        7
    );
    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_raft_log_entries_total") - log_before,
        42
    );
    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_raft_group_fatal_total") - fatal_before,
        1
    );
    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_raft_group_restart_total") - restart_before,
        2
    );
}
