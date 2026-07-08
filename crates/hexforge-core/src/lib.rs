//! hexforge-core — доменное ядро HexForge.
//!
//! Этот крейт не знает о Tauri, Wasmtime, файловой системе или UI.
//! Он определяет: (1) трейт `Transform`, единственный контракт для любой
//! операции трансформации данных (встроенной или из WASM-плагина),
//! (2) модель графа узлов (DAG), (3) модель истории как графа снапшотов.
//!
//! Инварианты, которые обязаны держать все зависящие крейты:
//! - Граф узлов всегда ациклический (проверяется в `graph::Graph::topo_order`).
//! - Любой `Transform::apply` — чистая синхронная функция без скрытого I/O.
//! - Любой снапшот воспроизводим: (input_content_hash, operation_id,
//!   operation_version, params) детерминированно определяют результат.

pub mod graph;
pub mod history;
pub mod registry;
pub mod transform;

pub use graph::{Graph, GraphError, NodeId, OperationNode};
pub use history::{Snapshot, SnapshotId};
pub use registry::TransformRegistry;
pub use transform::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
    Validate,
};
