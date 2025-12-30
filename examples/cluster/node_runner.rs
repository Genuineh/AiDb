//! Cluster node runner for integration testing
//!
//! Usage via env vars:
//!  NODE_ID - numeric node id (default 1)
//!  RAFT_ADDR - address for Raft gRPC server (default 0.0.0.0:50001)
//!  ADMIN_ADDR - address for simple admin TCP (default 0.0.0.0:8001)
//!  PEERS - comma-separated "id=http://host:port" (optional)
//!  INIT - if "1" or "true", call initialize with PEERS
//!  DB_DIR - directory for DB data (default /data/node<N>)

use aidb::cluster::{raft_storage, OpenRaftNode, RaftNetworkClientFactory, RaftNodeConfig};
use aidb::{Options, DB};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

fn parse_peers(s: &str) -> Vec<(u64, String)> {
    s.split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            let (id, addr) = entry.split_once('=')?;
            Some((id.parse().ok()?, addr.to_string()))
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let node_id: u64 = std::env::var("NODE_ID").unwrap_or_else(|_| "1".to_string()).parse()?;
    let raft_addr =
        std::env::var("RAFT_ADDR").unwrap_or_else(|_| format!("0.0.0.0:5000{}", node_id));
    let admin_addr =
        std::env::var("ADMIN_ADDR").unwrap_or_else(|_| format!("0.0.0.0:800{}", node_id));
    let peers = std::env::var("PEERS").unwrap_or_default();
    let init_flag = std::env::var("INIT").unwrap_or_default();
    let init_cluster = init_flag == "1" || init_flag.to_lowercase() == "true";
    let db_dir = std::env::var("DB_DIR").unwrap_or_else(|_| format!("/data/node{}", node_id));

    println!(
        "Starting node {}. Raft: {} Admin: {} DB: {}",
        node_id, raft_addr, admin_addr, db_dir
    );

    // Open DB
    let db = Arc::new(DB::open(&db_dir, Options::default())?);

    // Create node
    let network_factory = RaftNetworkClientFactory::new(node_id);
    let config = RaftNodeConfig { node_id, ..Default::default() };

    let node = Arc::new(OpenRaftNode::new(config, db.clone(), network_factory).await?);

    // Pre-populate the network factory with peer addresses from PEERS env var so
    // that this node can contact other nodes for votes/replication even before
    // membership changes are proposed/committed.
    if !peers.is_empty() {
        let parsed = parse_peers(&peers);
        for (id, addr) in parsed.into_iter() {
            node.add_node_address(id, addr.trim().to_string());
        }
    }

    // Start raft gRPC server
    let raft_socket: std::net::SocketAddr = raft_addr.parse()?;
    let node_clone = node.clone();
    tokio::spawn(async move {
        if let Err(e) = node_clone.start_server(raft_socket).await {
            eprintln!("Raft server error: {}", e);
        }
    });

    // Optionally initialize cluster
    if init_cluster && !peers.is_empty() {
        // Check for existing raft state to avoid re-initializing a node that already has state
        let mut has_state = false;
        match db.get(b"raft:last_log_id") {
            Ok(Some(_)) => {
                println!("Detected existing raft:last_log_id in DB; skipping INIT to avoid reinitialization");
                has_state = true;
            }
            Ok(None) => {}
            Err(e) => eprintln!("Warning: failed to read raft:last_log_id: {}", e),
        }
        if !has_state {
            match db.get(b"raft:last_applied") {
                Ok(Some(_)) => {
                    println!("Detected existing raft:last_applied in DB; skipping INIT to avoid reinitialization");
                    has_state = true;
                }
                Ok(None) => {}
                Err(e) => eprintln!("Warning: failed to read raft:last_applied: {}", e),
            }
        }
        if !has_state {
            match db.get(b"raft:membership") {
                Ok(Some(_)) => {
                    println!("Detected existing raft:membership in DB; skipping INIT to avoid reinitialization");
                    has_state = true;
                }
                Ok(None) => {}
                Err(e) => eprintln!("Warning: failed to read raft:membership: {}", e),
            }
        }

        if has_state {
            println!("Node has existing Raft state; resuming without calling INIT");
        } else {
            println!("Initializing cluster with peers: {}", peers);
            let parsed = parse_peers(&peers);
            let nodes: Vec<(u64, String)> =
                parsed.into_iter().map(|(id, addr)| (id, addr.trim().to_string())).collect();
            node.initialize(nodes).await?;
            println!("Cluster init requested")
        }
    }

    // Admin listener
    let listener = TcpListener::bind(admin_addr).await?;
    println!("Admin listener running");

    loop {
        let (stream, peer) = listener.accept().await?;
        let node_c = node.clone();
        let db_c = db.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(stream, node_c, db_c).await {
                eprintln!("Admin handler error from {:?}: {}", peer, e);
            }
        });
    }
}

async fn handle_conn(
    mut stream: TcpStream,
    node: Arc<OpenRaftNode>,
    db: Arc<DB>,
) -> Result<(), Box<dyn std::error::Error>> {
    let peer = stream.peer_addr()?;
    let (r, mut w) = stream.split();
    let mut reader = BufReader::new(r);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            break; // EOF
        }
        let cmd = line.trim();
        if cmd.is_empty() {
            continue;
        }
        let mut parts = cmd.splitn(3, ' ');
        let op = parts.next().unwrap_or("");

        // Log the received admin command for debugging
        println!("Admin command from {}: {}", peer, cmd.trim());

        match op.to_uppercase().as_str() {
            "INIT" => {
                if let Some(arg) = parts.next() {
                    let peers = parse_peers(arg);
                    let nodes: Vec<(u64, String)> = peers.into_iter().collect();
                    match node.initialize(nodes).await {
                        Ok(_) => w.write_all(b"OK\n").await?,
                        Err(e) => w.write_all(format!("ERR {}\n", e).as_bytes()).await?,
                    }
                } else {
                    w.write_all(b"ERR missing peers\n").await?;
                }
            }
            "ADD_LEARNER" => {
                if let Some(arg) = parts.next() {
                    let mut kv = arg.splitn(2, '=');
                    if let (Some(id_s), Some(addr)) = (kv.next(), kv.next()) {
                        if let Ok(id) = id_s.parse::<u64>() {
                            match node.add_learner(id, addr.to_string()).await {
                                Ok(_) => w.write_all(b"OK\n").await?,
                                Err(e) => w.write_all(format!("ERR {}\n", e).as_bytes()).await?,
                            }
                        } else {
                            w.write_all(b"ERR invalid id\n").await?;
                        }
                    } else {
                        w.write_all(b"ERR bad arg\n").await?;
                    }
                } else {
                    w.write_all(b"ERR missing arg\n").await?;
                }
            }
            "CHANGE_MEMBERS" => {
                if let Some(arg) = parts.next() {
                    let ids: Vec<u64> = arg.split(',').filter_map(|s| s.parse().ok()).collect();
                    match node.change_membership(ids).await {
                        Ok(_) => w.write_all(b"OK\n").await?,
                        Err(e) => w.write_all(format!("ERR {}\n", e).as_bytes()).await?,
                    }
                } else {
                    w.write_all(b"ERR missing ids\n").await?;
                }
            }
            "PUT" => {
                if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                    match node.put(key.as_bytes().to_vec(), value.as_bytes().to_vec()).await {
                        Ok(_) => w.write_all(b"OK\n").await?,
                        Err(e) => w.write_all(format!("ERR {}\n", e).as_bytes()).await?,
                    }
                } else {
                    w.write_all(b"ERR missing key/value\n").await?;
                }
            }
            "DELETE" => {
                if let Some(key) = parts.next() {
                    match node.delete(key.as_bytes().to_vec()).await {
                        Ok(_) => w.write_all(b"OK\n").await?,
                        Err(e) => w.write_all(format!("ERR {}\n", e).as_bytes()).await?,
                    }
                } else {
                    w.write_all(b"ERR missing key\n").await?;
                }
            }
            "GET" => {
                if let Some(key) = parts.next() {
                    // State machine data is stored with "sm:" prefix
                    let sm_key = format!("sm:{}", key);
                    match db.get(sm_key.as_bytes()) {
                        Ok(Some(val)) => {
                            // return as utf8 if possible, else base64
                            if let Ok(s) = String::from_utf8(val.clone()) {
                                w.write_all(format!("OK {}\n", s).as_bytes()).await?;
                            } else {
                                w.write_all(
                                    format!("OK base64:{}\n", base64::encode(&val)).as_bytes(),
                                )
                                .await?;
                            }
                        }
                        Ok(None) => w.write_all(b"OK None\n").await?,
                        Err(e) => w.write_all(format!("ERR {}\n", e).as_bytes()).await?,
                    }
                } else {
                    w.write_all(b"ERR missing key\n").await?;
                }
            }
            "IS_LEADER" => {
                let is_leader = node.is_leader().await;
                w.write_all(format!("OK {}\n", is_leader).as_bytes()).await?;
            }
            "LEADER" => {
                if let Some(l) = node.get_leader().await {
                    w.write_all(format!("OK {}\n", l).as_bytes()).await?;
                } else {
                    w.write_all(b"OK None\n").await?;
                }
            }
            "METRICS" => {
                let m = node.metrics().await;
                w.write_all(
                    format!(
                        "OK term={} leader={:?} last_log_index={:?} state={:?}\n",
                        m.current_term, m.current_leader, m.last_log_index, m.state
                    )
                    .as_bytes(),
                )
                .await?;
            }
            "ADDRS" => {
                let addrs = node.node_addresses().await;
                if addrs.is_empty() {
                    w.write_all(b"OK no addresses\n").await?;
                } else {
                    for (id, addr) in addrs.into_iter() {
                        w.write_all(format!("ADDR {} {}\n", id, addr).as_bytes()).await?;
                    }
                }
            }
            "ADD_ADDR" => {
                if let Some(arg) = parts.next() {
                    let mut kv = arg.splitn(2, '=');
                    if let (Some(id_s), Some(addr)) = (kv.next(), kv.next()) {
                        if let Ok(id) = id_s.parse::<u64>() {
                            node.add_node_address(id, addr.to_string());
                            w.write_all(b"OK\n").await?;
                        } else {
                            w.write_all(b"ERR invalid id\n").await?;
                        }
                    } else {
                        w.write_all(b"ERR bad arg\n").await?;
                    }
                } else {
                    w.write_all(b"ERR missing arg\n").await?;
                }
            }
            "MEMBERS" => {
                // Read last applied and membership from DB to report committed membership state
                let mut last_applied_idx = String::from("None");
                match db.get(b"raft:last_applied") {
                    Ok(Some(data)) => {
                        if let Ok(log_id) = bincode::deserialize::<openraft::LogId<u64>>(&data) {
                            last_applied_idx = format!("{}", log_id.index);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        w.write_all(format!("ERR failed to read last_applied: {}\n", e).as_bytes())
                            .await?;
                        continue;
                    }
                }

                let mut mem_debug = String::from("None");
                match db.get(b"raft:membership") {
                    Ok(Some(data)) => {
                        if let Ok(mem) = bincode::deserialize::<
                            openraft::StoredMembership<u64, openraft::BasicNode>,
                        >(&data)
                        {
                            mem_debug = format!("{:?}", mem);
                        } else {
                            mem_debug = String::from("ERR deser membership");
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        w.write_all(format!("ERR failed to read membership: {}\n", e).as_bytes())
                            .await?;
                        continue;
                    }
                }

                w.write_all(
                    format!(
                        "OK last_applied_index={} membership_debug={}\n",
                        last_applied_idx, mem_debug
                    )
                    .as_bytes(),
                )
                .await?;
            }
            "SHUTDOWN" => match node.shutdown().await {
                Ok(_) => {
                    w.write_all(b"OK shutting down\n").await?;
                    break;
                }
                Err(e) => w.write_all(format!("ERR {}\n", e).as_bytes()).await?,
            },
            "DUMP_LOG" => {
                if let Some(idx_s) = parts.next() {
                    if let Ok(idx) = idx_s.parse::<u64>() {
                        let key = format!("raft:log:{}", idx);
                        match db.get(key.as_bytes()) {
                            Ok(Some(data)) => {
                                // try to deserialize with rmp_serde
                                match rmp_serde::from_slice::<
                                    openraft::Entry<raft_storage::TypeConfig>,
                                >(&data)
                                {
                                    Ok(entry) => {
                                        let info = format!("LOG {}: {:?}\n", idx, entry.log_id);
                                        w.write_all(info.as_bytes()).await?;
                                        match entry.payload {
                                            openraft::EntryPayload::Membership(m) => {
                                                w.write_all(
                                                    format!("  payload: Membership({:?})\n", m)
                                                        .as_bytes(),
                                                )
                                                .await?;
                                            }
                                            openraft::EntryPayload::Normal(req) => {
                                                w.write_all(
                                                    format!("  payload: Normal({:?})\n", req)
                                                        .as_bytes(),
                                                )
                                                .await?;
                                            }
                                            openraft::EntryPayload::Blank => {
                                                w.write_all(b"  payload: Blank\n").await?;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        w.write_all(
                                            format!(
                                                "ERR failed to deserialize log {}: {}\n",
                                                idx, e
                                            )
                                            .as_bytes(),
                                        )
                                        .await?;
                                    }
                                }
                            }
                            Ok(None) => {
                                w.write_all(format!("OK no log {}\n", idx).as_bytes()).await?;
                            }
                            Err(e) => {
                                w.write_all(format!("ERR reading log {}: {}\n", idx, e).as_bytes())
                                    .await?;
                            }
                        }
                    } else {
                        w.write_all(b"ERR invalid index\n").await?;
                    }
                } else {
                    w.write_all(b"ERR missing index\n").await?;
                }
            }
            "DUMP_LOG_RANGE" => {
                if let (Some(start_s), Some(end_s)) = (parts.next(), parts.next()) {
                    if let (Ok(start), Ok(end)) = (start_s.parse::<u64>(), end_s.parse::<u64>()) {
                        for i in start..=end {
                            let cmd = format!("DUMP_LOG {}\n", i);
                            w.write_all(cmd.as_bytes()).await?; // echo command for clarity
                                                                // reuse the same handler by reading the DB directly
                            let key = format!("raft:log:{}", i);
                            match db.get(key.as_bytes()) {
                                Ok(Some(data)) => {
                                    match rmp_serde::from_slice::<
                                        openraft::Entry<raft_storage::TypeConfig>,
                                    >(&data)
                                    {
                                        Ok(entry) => {
                                            w.write_all(
                                                format!("LOG {}: {:?}\n", i, entry.log_id)
                                                    .as_bytes(),
                                            )
                                            .await?;
                                            match entry.payload {
                                                openraft::EntryPayload::Membership(m) => {
                                                    w.write_all(
                                                        format!("  payload: Membership({:?})\n", m)
                                                            .as_bytes(),
                                                    )
                                                    .await?;
                                                }
                                                openraft::EntryPayload::Normal(req) => {
                                                    w.write_all(
                                                        format!("  payload: Normal({:?})\n", req)
                                                            .as_bytes(),
                                                    )
                                                    .await?;
                                                }
                                                openraft::EntryPayload::Blank => {
                                                    w.write_all(b"  payload: Blank\n").await?;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            w.write_all(
                                                format!(
                                                    "ERR failed to deserialize log {}: {}\n",
                                                    i, e
                                                )
                                                .as_bytes(),
                                            )
                                            .await?;
                                        }
                                    }
                                }
                                Ok(None) => {
                                    w.write_all(format!("OK no log {}\n", i).as_bytes()).await?;
                                }
                                Err(e) => {
                                    w.write_all(
                                        format!("ERR reading log {}: {}\n", i, e).as_bytes(),
                                    )
                                    .await?;
                                }
                            }
                        }
                    } else {
                        w.write_all(b"ERR invalid start/end\n").await?;
                    }
                } else {
                    w.write_all(b"ERR missing args\n").await?;
                }
            }
            _ => {
                w.write_all(b"ERR unknown command\n").await?;
            }
        }
    }

    println!("Closed admin connection from {}", peer);
    Ok(())
}
