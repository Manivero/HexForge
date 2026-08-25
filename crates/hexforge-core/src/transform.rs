//! Центральный контракт ядра: трейт `Transform`.
//!
//! Любая операция трансформации данных в HexForge — встроенная (`hexforge-ops`)
//! или загруженная как подписанный WASM-плагин через `hexforge-plugin-host` —
//! реализует этот трейт (либо изоморфный ему WIT-интерфейс на стороне плагина).
//! Ядро ничего не знает о конкретных операциях: реестр (`registry.rs`) хранит
//! их исключительно как `&dyn Transform`.

use serde::{Deserialize, Serialize};
use std::any::Any;
use std::borrow::Cow;

/// Единица данных, которой оперирует `Transform`. `Cow` позволяет операциям,
/// которые не модифицируют байты (напр. no-op передача), избежать копирования —
/// прямое следствие требования zero-copy slicing из NFR-2.
pub type ByteView<'a> = Cow<'a, [u8]>;

/// Декларация свойств операции. Используется планировщиком стриминга
/// (`hexforge-stream`) и UI (оценка памяти, предупреждения FR-5.3)
/// без необходимости выполнять саму операцию.
///
/// `camelCase` обязателен: тип уходит на провод в `list_operations` и
/// побайтово соответствует `TransformCapabilities` из `ipc-contract.ts`
/// (`memoryCost`, а не `memory_cost`) — см. `05-IPC-CONTRACT.md`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransformCapabilities {
    /// Одинаковый вход + одинаковые параметры => гарантированно одинаковый выход.
    /// `false` для операций вроде "generate random key" — такие узлы обязаны
    /// фиксировать использованное значение в снапшоте, а не только параметры.
    pub deterministic: bool,
    /// Операция умеет обрабатывать вход по чанкам без полного буфера в памяти.
    pub streamable: bool,
    /// Верхняя граница потребления памяти относительно размера входа.
    pub memory_cost: MemoryCost,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCost {
    /// O(1) относительно размера входа (напр. потоковый hash).
    Constant,
    /// O(chunk_size), не зависит от полного размера входа.
    PerChunk,
    /// O(n) — вся операция требует полный буфер в памяти.
    FullBuffer,
}

/// Единая ошибка выполнения для всех реализаций `Transform` — позволяет UI
/// унифицированно рендерить диагностику независимо от того, какая именно
/// операция упала.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum TransformError {
    #[error("invalid parameter '{field}': {reason}")]
    InvalidParameter { field: String, reason: String },
    #[error("input is not valid for this operation: {reason}")]
    InvalidInput { reason: String },
    #[error("operation exceeded memory budget: {limit_mb}MB")]
    MemoryBudgetExceeded { limit_mb: u64 },
    #[error("internal error: {0}")]
    Internal(String),
}

/// Контекст выполнения, передаваемый планировщиком в `Transform::apply*`.
/// Инкапсулирует прогресс-репортинг и кооперативную отмену без завязки
/// реализаций операций на конкретный async-рантайм.
pub trait ExecutionContext: Send + Sync {
    fn report_progress(&self, bytes_processed: u64, bytes_total: Option<u64>);
    fn is_cancelled(&self) -> bool;
}

/// Контекст-заглушка для юнит-тестов и CLI-режима без UI-прогресса.
pub struct NullExecutionContext;

impl ExecutionContext for NullExecutionContext {
    fn report_progress(&self, _bytes_processed: u64, _bytes_total: Option<u64>) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Опциональная предварительная валидация параметров — отдельно от `apply`,
/// чтобы UI могло подсвечивать ошибки формы без запуска операции
/// (мгновенная обратная связь, NFR-1).
pub trait Validate {
    fn validate(&self, params: &serde_json::Value) -> Result<(), Vec<TransformError>>;
}

/// Центральный трейт ядра.
pub trait Transform: Send + Sync {
    /// Стабильный идентификатор операции, напр. "encoding.base64.decode".
    /// Формат: `<category>.<name>.<variant>`, неизменяем между версиями —
    /// именно `id` + `version` фиксируются в снапшотах истории для
    /// воспроизводимости (FR-4.2).
    fn id(&self) -> &'static str;

    /// Semver-версия конкретной реализации.
    fn version(&self) -> &'static str;

    /// Человекочитаемое имя для UI (Command Palette, инспектор узла).
    fn display_name(&self) -> &'static str;

    /// Категория для группировки в Command Palette / реестре операций.
    fn category(&self) -> &'static str;

    /// JSON Schema параметров — фронтенд рендерит форму параметров
    /// автоматически на основе этой схемы (FR-3.2).
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    fn capabilities(&self) -> TransformCapabilities;

    /// Разовое (не потоковое) выполнение над полным буфером.
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        params: &serde_json::Value,
        ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError>;

    /// Потоковое выполнение — планировщик вызывает эту функцию чанк за чанком
    /// для операций с `capabilities().streamable == true`. `state` — per-node
    /// состояние между вызовами, принадлежащее планировщику (`Box<dyn Any>`):
    /// операция при первом вызове обязана засеять свой конкретный тип
    /// (`*state = Box::new(MyState::default())`) и далее работать через
    /// `downcast_mut` — контракт docs/04 §6 ("заведённое и освобождаемое
    /// планировщиком") сохраняется, но тип выбирает операция.
    ///
    /// Дефолтная реализация осознанно не поддерживается: операция обязана
    /// либо явно реализовать потоковый путь, либо задекларировать
    /// `streamable: false` и полагаться на `apply` над аккумулированным
    /// планировщиком буфером (см. `04-RUST-CORE-ARCHITECTURE.md`, §6).
    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn Any + Send>,
        params: &serde_json::Value,
        ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        let _ = (chunk, is_last, state, params, ctx);
        Err(TransformError::Internal(
            "apply_chunk not implemented for this operation; capabilities().streamable must be false".into(),
        ))
    }
}

/// Контракт N-арных операций слияния (PRD FR-1.2/FR-1.4: `Merge`/`XOR`,
/// «concat, xor, diff, zip»). Семантика слияния принадлежит операции,
/// а не планировщику: узел графа с `inputs.len() > 1` исполним только тогда,
/// когда его операция реализует этот трейт; иначе планировщик возвращает
/// `InvalidInput`. Порядок `inputs` в узле — часть контракта операции
/// (напр. concat склеивает ровно в заявленном порядке).
///
/// Трейт отдельный и опциональный: унарные операции не трогаются, реестр
/// хранит merge-реализации во второй карте (`TransformRegistry::get_merge`).
pub trait MergeTransform: Transform {
    fn apply_merge<'a>(
        &self,
        inputs: Vec<ByteView<'a>>,
        params: &serde_json::Value,
        ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError>;
}
