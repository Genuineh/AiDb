use aidb::cluster::metrics;

#[test]
fn test_raft_metrics_register_and_record() {
    metrics::init();
    let registry = prometheus::Registry::new();
    metrics::register_into(&registry).expect("register raft metrics");

    metrics::record_raft_rpc("vote", "incoming");
    metrics::record_raft_log_entries(3);

    let families = registry.gather();
    let rpc = families
        .iter()
        .find(|f| f.get_name() == "aidb_raft_rpc_total")
        .expect("aidb_raft_rpc_total");
    assert_eq!(
        rpc.get_metric()[0].get_counter().get_value(),
        1.0,
        "vote incoming"
    );

    let logs = families
        .iter()
        .find(|f| f.get_name() == "aidb_raft_log_entries_total")
        .expect("aidb_raft_log_entries_total");
    assert_eq!(logs.get_metric()[0].get_counter().get_value(), 3.0);
}
