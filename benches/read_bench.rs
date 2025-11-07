// Read performance benchmarks for AiDb

use aidb::{Options, DB};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use tempfile::TempDir;

fn benchmark_sequential_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_read");

    for size in [100, 1000, 10000].iter() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Pre-populate data
        for i in 0..*size {
            let key = format!("key{:08}", i);
            let value = format!("value{:08}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap();

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                for i in 0..size {
                    let key = format!("key{:08}", i);
                    let value = db.get(key.as_bytes()).unwrap();
                    black_box(value);
                }
            });
        });
    }

    group.finish();
}

fn benchmark_random_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_read");

    for size in [100, 1000, 10000].iter() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Pre-populate data
        for i in 0..*size {
            let key = format!("key{:08}", i);
            let value = format!("value{:08}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap();

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                use rand::Rng;
                let mut rng = rand::rng();

                for _ in 0..size {
                    let key_num: usize = rng.random_range(0..size);
                    let key = format!("key{:08}", key_num);
                    let value = db.get(key.as_bytes()).unwrap();
                    black_box(value);
                }
            });
        });
    }

    group.finish();
}

fn benchmark_cache_hit(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_hit");

    let temp_dir = TempDir::new().unwrap();
    let db = DB::open(temp_dir.path(), Options::default()).unwrap();

    // Pre-populate data
    for i in 0..1000 {
        let key = format!("key{:08}", i);
        let value = format!("value{:08}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }
    db.flush().unwrap();

    // Warm up cache by reading all keys once
    for i in 0..1000 {
        let key = format!("key{:08}", i);
        let _ = db.get(key.as_bytes()).unwrap();
    }

    group.throughput(Throughput::Elements(1000));
    group.bench_function("cached_reads", |b| {
        b.iter(|| {
            for i in 0..1000 {
                let key = format!("key{:08}", i);
                let value = db.get(key.as_bytes()).unwrap();
                black_box(value);
            }
        });
    });

    group.finish();
}

fn benchmark_read_missing_keys(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_missing");

    let temp_dir = TempDir::new().unwrap();
    let db = DB::open(temp_dir.path(), Options::default()).unwrap();

    // Pre-populate data with keys 0-999
    for i in 0..1000 {
        let key = format!("key{:08}", i);
        let value = format!("value{:08}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }
    db.flush().unwrap();

    group.throughput(Throughput::Elements(1000));
    group.bench_function("missing_keys", |b| {
        b.iter(|| {
            // Try to read keys 1000-1999 (which don't exist)
            for i in 1000..2000 {
                let key = format!("key{:08}", i);
                let value = db.get(key.as_bytes()).unwrap();
                black_box(value);
            }
        });
    });

    group.finish();
}

fn benchmark_read_with_bloom_filter(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_with_bloom");

    // Without bloom filter
    {
        let temp_dir = TempDir::new().unwrap();
        let mut opts = Options::default();
        opts.use_bloom_filter = false;
        let db = DB::open(temp_dir.path(), opts).unwrap();

        for i in 0..1000 {
            let key = format!("key{:08}", i);
            let value = format!("value{:08}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap();

        group.bench_function("without_bloom", |b| {
            b.iter(|| {
                // Try to read missing keys
                for i in 1000..2000 {
                    let key = format!("key{:08}", i);
                    let value = db.get(key.as_bytes()).unwrap();
                    black_box(value);
                }
            });
        });
    }

    // With bloom filter
    {
        let temp_dir = TempDir::new().unwrap();
        let opts = Options::default(); // Bloom filter enabled by default
        let db = DB::open(temp_dir.path(), opts).unwrap();

        for i in 0..1000 {
            let key = format!("key{:08}", i);
            let value = format!("value{:08}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap();

        group.bench_function("with_bloom", |b| {
            b.iter(|| {
                // Try to read missing keys
                for i in 1000..2000 {
                    let key = format!("key{:08}", i);
                    let value = db.get(key.as_bytes()).unwrap();
                    black_box(value);
                }
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_sequential_read,
    benchmark_random_read,
    benchmark_cache_hit,
    benchmark_read_missing_keys,
    benchmark_read_with_bloom_filter
);
criterion_main!(benches);

