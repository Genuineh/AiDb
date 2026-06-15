/// AiDb 集群模式示例 (需要 --features cluster).
///
/// 演示 CRC16 路由和多 Group 的基本使用.
///
/// 运行: cargo run --features cluster --example cluster
fn main() -> Result<(), Box<dyn std::error::Error>> {
  // CRC16 槽位计算
  #[cfg(feature = "cluster")]
  {
    let key = b"mykey";
    let slot = aidb::cluster::key_to_slot(key);
    println!("✅ key 'mykey' → slot {}", slot);

    // Hash tag 测试
    let tagged = b"{user:1001}.name";
    let tag_slot = aidb::cluster::key_to_slot(tagged);
    println!("✅ key '{{user:1001}}.name' → slot {}", tag_slot);

    // Hash tag 提取
    let tag = aidb::cluster::extract_hash_tag(tagged);
    let tag_str = if tag.is_empty() {
      "(none)".to_string()
    } else {
      String::from_utf8_lossy(tag).to_string()
    };
    println!("✅ hash tag: {tag_str:?}");
  }

  #[cfg(not(feature = "cluster"))]
  {
    println!("ℹ️  请使用 --features cluster 构建此示例");
  }

  Ok(())
}
