//! @component aidb-cluster

use aidb::cluster::RaftNetworkClientFactory;

#[test]
fn test_network_factory_creation() {
    let factory = RaftNetworkClientFactory::new(1, 1, 30, 65 * 1024 * 1024);
    assert_eq!(factory.list_nodes().len(), 0);
}

#[test]
fn test_add_remove_node() {
    let factory = RaftNetworkClientFactory::new(1, 1, 30, 65 * 1024 * 1024);
    factory.add_node(2, "http://127.0.0.1:50002".to_string());
    assert_eq!(factory.list_nodes().len(), 1);
    factory.remove_node(2);
    assert_eq!(factory.list_nodes().len(), 0);
}
