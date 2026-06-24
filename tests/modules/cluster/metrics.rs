use aidb::cluster::metrics;
use aidb::metrics::testutil;

#[test]
fn test_raft_metrics_otel_record() {
    let exporter = testutil::init_in_memory();
    let rpc_before = testutil::counter_sum(&exporter, "aidb_raft_rpc_total");
    let logs_before = testutil::counter_sum(&exporter, "aidb_raft_log_entries_total");

    metrics::record_raft_rpc("vote", "incoming");
    metrics::record_raft_log_entries(3);

    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_raft_rpc_total") - rpc_before,
        1,
        "vote incoming"
    );
    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_raft_log_entries_total") - logs_before,
        3
    );
}
