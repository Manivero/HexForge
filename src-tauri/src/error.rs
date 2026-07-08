//! Зеркало `HexForgeError`/`HexForgeErrorKind` из `src/lib/ipc-contract.ts`.
//! Любое расхождение полей между этим файлом и TS-контрактом — баг
//! (см. `05-IPC-CONTRACT.md`, §0 про `check-ipc-parity`).

use hexforge_core::{GraphError, TransformError};
use serde::Serialize;

/// Было: `kind: &'static str`, заполнявшееся литералами в 8 разных местах
/// без единой точки проверки — опечатка в любом из них ("InvalidInpt")
/// расходится с TS-объединением `HexForgeErrorKind` молча, без ошибки
/// компиляции ни на одной из сторон моста. Перечисление даёт то же самое
/// значение на проводе (`#[serde(rename_all = "PascalCase")]` даёт ровно
/// те же строки: "InvalidParameter", "InvalidInput", ...), но опечатка
/// теперь — ошибка компиляции Rust, а не молчаливое расхождение контракта.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum HexForgeErrorKind {
    InvalidParameter,
    InvalidInput,
    MemoryBudgetExceeded,
    CycleDetected,
    DanglingInput,
    PluginSignatureInvalid,
    PluginCapabilityDenied,
    Internal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HexForgeError {
    pub kind: HexForgeErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_mb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

impl HexForgeError {
    /// Конструкторы ниже — единственная причина существования этого impl:
    /// раньше каждый call site в `commands.rs` вручную заполнял
    /// `field: None, limit_mb: None, node_id: None`, что на практике
    /// означало 3 лишние строки шаблонного кода на каждую ошибку и
    /// реальный риск однажды забыть проставить нужное поле (напр.
    /// `node_id` при `Internal`-ошибке внутри `resolve_node_output`).
    pub fn invalid_parameter(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: HexForgeErrorKind::InvalidParameter,
            message: message.into(),
            field: Some(field.into()),
            limit_mb: None,
            node_id: None,
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            kind: HexForgeErrorKind::InvalidInput,
            message: message.into(),
            field: None,
            limit_mb: None,
            node_id: None,
        }
    }

    pub fn memory_budget_exceeded(limit_mb: u64) -> Self {
        Self {
            kind: HexForgeErrorKind::MemoryBudgetExceeded,
            message: format!("operation exceeded memory budget: {limit_mb}MB"),
            field: None,
            limit_mb: Some(limit_mb),
            node_id: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: HexForgeErrorKind::Internal,
            message: message.into(),
            field: None,
            limit_mb: None,
            node_id: None,
        }
    }

    /// `internal` с привязкой к узлу — самый частый случай в `run_node`/
    /// `resolve_node_output`, где почти каждая ошибка обязана нести
    /// `node_id` для подсветки конкретного узла в UI.
    pub fn internal_for_node(node_id: impl std::fmt::Display, message: impl Into<String>) -> Self {
        Self {
            kind: HexForgeErrorKind::Internal,
            message: message.into(),
            field: None,
            limit_mb: None,
            node_id: Some(node_id.to_string()),
        }
    }
}

impl From<TransformError> for HexForgeError {
    fn from(err: TransformError) -> Self {
        match err {
            TransformError::InvalidParameter { field, reason } => {
                HexForgeError::invalid_parameter(field, reason)
            }
            TransformError::InvalidInput { reason } => HexForgeError::invalid_input(reason),
            TransformError::MemoryBudgetExceeded { limit_mb } => {
                HexForgeError::memory_budget_exceeded(limit_mb)
            }
            TransformError::Internal(msg) => HexForgeError::internal(msg),
        }
    }
}

impl From<GraphError> for HexForgeError {
    fn from(err: GraphError) -> Self {
        match err {
            GraphError::CycleDetected => HexForgeError {
                kind: HexForgeErrorKind::CycleDetected,
                message: "graph contains a cycle".into(),
                field: None,
                limit_mb: None,
                node_id: None,
            },
            GraphError::DanglingInput(node_id) => HexForgeError {
                kind: HexForgeErrorKind::DanglingInput,
                message: format!("node {node_id} references an unknown input"),
                field: None,
                limit_mb: None,
                node_id: Some(node_id.to_string()),
            },
        }
    }
}

pub type HexForgeResult<T> = Result<T, HexForgeError>;
