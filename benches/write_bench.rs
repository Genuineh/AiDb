// Write performance benchmarks for AiDb

use aidb::{Options, WriteBatch, DB};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use tempfile::TempDir;

fn benchmark_sequential_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_write");

    for size in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let temp_dir = TempDir::new().unwrap();
                let db = DB::open(temp_dir.path(), Options::default()).unwrap();

                for i in 0..size {
                    let key = format!("key{:08}", i);
                    let value = format!("value{:08}", i);
                    db.put(key.as_bytes(), value.as_bytes()).unwrap();
                }

                black_box(&db);
            });
        });
    }

    group.finish();
}

fn benchmark_random_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_write");

    for size in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let temp_dir = TempDir::new().unwrap();
                let db = DB::open(temp_dir.path(), Options::default()).unwrap();

                use rand::Rng;
                let mut rng = rand::rng();

                for _ in 0..size {
                    let key_num: u32 = rng.random();
                    let key = format!("key{:08}", key_num);
                    let value = format!("value{:08}", key_num);
                    db.put(key.as_bytes(), value.as_bytes()).unwrap();
                }

                black_box(&db);
            });
        });
    }

    group.finish();
}

fn benchmark_batch_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_write");

    for batch_size in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &batch_size| {
                b.iter(|| {
                    let temp_dir = TempDir::new().unwrap();
                    let db = DB::open(temp_dir.path(), Options::default()).unwrap();

                    let mut batch = WriteBatch::new();
                    for i in 0..batch_size {
                        let key = format!("key{:08}", i);
                        let value = format!("value{:08}", i);
                        batch.put(key.as_bytes(), value.as_bytes());
                    }

                    db.write(batch).unwrap();

                    black_box(&db);
                });
            },
        );
    }

    group.finish();
}

fn benchmark_overwrite(c: &mut Criterion) {
    let mut group = c.benchmark_group("overwrite");

    group.throughput(Throughput::Elements(1000));
    group.bench_function("overwrite_1000", |b| {
        // Setup database once for all iterations
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Pre-populate with data
        for i in 0..1000 {
            let key = format!("key{:08}", i);
            let value = format!("initial_value{:08}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }

        b.iter(|| {
            for i in 0..1000 {
                let key = format!("key{:08}", i);
                let value = format!("updated_value{:08}", i);
                db.put(key.as_bytes(), value.as_bytes()).unwrap();
            }
            black_box(&db);
        });
    });

    group.finish();
}

fn benchmark_write_with_compression(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_with_compression");

    // Benchmark with no compression
    group.bench_function("no_compression", |b| {
        b.iter(|| {
            let temp_dir = TempDir::new().unwrap();
            let opts = Options::default().compression(aidb::config::CompressionType::None);
            let db = DB::open(temp_dir.path(), opts).unwrap();

            for i in 0..1000 {
                let key = format!("key{:08}", i);
                let value = vec![b'x'; 100]; // 100 bytes of repeating data
                db.put(key.as_bytes(), &value).unwrap();
            }

            black_box(&db);
        });
    });

    // Benchmark with Snappy compression
    #[cfg(feature = "snappy")]
    group.bench_function("snappy_compression", |b| {
        b.iter(|| {
            let temp_dir = TempDir::new().unwrap();
            let opts = Options::default().compression(aidb::config::CompressionType::Snappy);
            let db = DB::open(temp_dir.path(), opts).unwrap();

            for i in 0..1000 {
                let key = format!("key{:08}", i);
                let value = vec![b'x'; 100]; // 100 bytes of repeating data
                db.put(key.as_bytes(), &value).unwrap();
            }

            black_box(&db);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_sequential_write,
    benchmark_random_write,
    benchmark_batch_write,
    benchmark_overwrite,
    benchmark_write_with_compression
);
criterion_main!(benches);
