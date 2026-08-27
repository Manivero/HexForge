use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use hexforge_stream::{chunk_ranges, DEFAULT_CHUNK_SIZE_BYTES};

fn bench_chunk_ranges(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunk_ranges");
    for size in [1024, 64 * 1024, 1024 * 1024, 64 * 1024 * 1024] {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(format!("{size}B"), &size, |b, &size| {
            b.iter(|| chunk_ranges(black_box(size), black_box(DEFAULT_CHUNK_SIZE_BYTES)))
        });
    }
    group.finish();
}

fn bench_chunk_64m(c: &mut Criterion) {
    c.bench_function("chunk_64M_default", |b| {
        b.iter(|| chunk_ranges(black_box(64 * 1024 * 1024 + 12345), black_box(DEFAULT_CHUNK_SIZE_BYTES)))
    });
}

criterion_group!(benches, bench_chunk_ranges, bench_chunk_64m);
criterion_main!(benches);
