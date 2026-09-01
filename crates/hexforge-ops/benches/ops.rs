use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use hexforge_core::Transform;
use std::borrow::Cow;

fn bench_base64(c: &mut Criterion) {
    let mut group = c.benchmark_group("base64");
    for size in [1024, 64 * 1024, 1024 * 1024] {
        let data = vec![b'A'; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(format!("encode_{size}B"), &data, |b, data| {
            let op = hexforge_ops::encoding::base64::Base64Encode;
            let ctx = hexforge_core::transform::NullExecutionContext;
            b.iter(|| {
                op.apply(Cow::Borrowed(black_box(data)), &serde_json::json!({}), &ctx)
                    .unwrap()
            })
        });
        let encoded = {
            let op = hexforge_ops::encoding::base64::Base64Encode;
            let ctx = hexforge_core::transform::NullExecutionContext;
            op.apply(Cow::Borrowed(&data), &serde_json::json!({}), &ctx)
                .unwrap()
                .into_owned()
        };
        group.bench_with_input(format!("decode_{size}B"), &encoded, |b, encoded| {
            let op = hexforge_ops::encoding::base64::Base64Decode;
            let ctx = hexforge_core::transform::NullExecutionContext;
            b.iter(|| {
                op.apply(
                    Cow::Borrowed(black_box(encoded)),
                    &serde_json::json!({}),
                    &ctx,
                )
                .unwrap()
            })
        });
    }
    group.finish();
}

fn bench_sha256(c: &mut Criterion) {
    let mut group = c.benchmark_group("sha256");
    for size in [1024, 64 * 1024, 1024 * 1024] {
        let data = vec![b'x'; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(format!("hash_{size}B"), &data, |b, data| {
            let op = hexforge_ops::hashing::Sha256Hash;
            let ctx = hexforge_core::transform::NullExecutionContext;
            b.iter(|| {
                op.apply(Cow::Borrowed(black_box(data)), &serde_json::json!({}), &ctx)
                    .unwrap()
            })
        });
        // Chunked vs non-chunked for 64 MiB - 1 byte would be large, use 1 MiB chunked
        if size == 1024 * 1024 {
            group.bench_with_input("hash_chunked_1MiB", &data, |b, data| {
                let op = hexforge_ops::hashing::Sha256Hash;
                let ctx = hexforge_core::transform::NullExecutionContext;
                let mut state: Box<dyn std::any::Any + Send> = Box::new(());
                b.iter(|| {
                    // Simulate chunked apply with 64 KiB chunks (to avoid 64 MiB alloc per iter)
                    let chunk_size = 64 * 1024;
                    let mut out = Vec::new();
                    for (i, chunk) in data.chunks(chunk_size).enumerate() {
                        let is_last = i == data.len() / chunk_size;
                        out.extend(
                            op.apply_chunk(
                                black_box(chunk),
                                is_last,
                                &mut state,
                                &serde_json::json!({}),
                                &ctx,
                            )
                            .unwrap(),
                        );
                    }
                    out
                })
            });
        }
    }
    group.finish();
}

fn bench_rot13(c: &mut Criterion) {
    let data = vec![b'a'; 1024 * 1024];
    c.benchmark_group("rot13")
        .throughput(Throughput::Bytes(data.len() as u64))
        .bench_function("rot13_1MiB", |b| {
            let op = hexforge_ops::text::rot13::Rot13;
            let ctx = hexforge_core::transform::NullExecutionContext;
            b.iter(|| {
                op.apply(
                    Cow::Borrowed(black_box(&data)),
                    &serde_json::json!({}),
                    &ctx,
                )
                .unwrap()
            })
        });
}

criterion_group!(benches, bench_base64, bench_sha256, bench_rot13);
criterion_main!(benches);
