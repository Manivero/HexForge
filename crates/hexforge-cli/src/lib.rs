//! hexforge-cli — headless-режим HexForge (PRD FR-7.3): запуск рецепта
//! `.hexforge` (формат = GraphDto, тот же, что пишет export_recipe) над
//! входным файлом с записью результата. Тот же движок, что и GUI:
//! hexforge-engine + hexforge-ops.
//!
//! ```text
//! hexforge-cli run recipe.hexforge --in input.bin --out output.bin
//! ```

use hexforge_core::graph::NodeId;
use hexforge_engine::graph_dto::{validate_graph, GraphDto};
use hexforge_engine::state::{AppState, SourceEntry};
use serde_json::json;
use std::collections::HashSet;

/// Итог успешного запуска рецепта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub output_bytes: u64,
    pub duration_ms: u64,
    /// Число исполненных узлов цепочки (по журналу истории).
    pub executed_nodes: usize,
}

fn validate_cli_path(path: &str, field: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err(format!("{field} path must not be empty"));
    }
    if path.len() > 4096 {
        return Err(format!("{field} path exceeds maximum length (4096)"));
    }
    if path.contains('\0') {
        return Err(format!("{field} path contains null byte"));
    }
    Ok(())
}

/// Ошибки CLI: человекочитаемая строка уходит в stderr / тестовый assert.
pub fn run_recipe(
    recipe_path: &str,
    in_paths: &[String],
    out_path: &str,
) -> Result<RunSummary, String> {
    validate_cli_path(recipe_path, "recipe")?;
    for p in in_paths {
        validate_cli_path(p, "input")?;
    }
    validate_cli_path(out_path, "output")?;
    if in_paths.is_empty() {
        return Err("at least one --in <file> is required".into());
    }
    let started = std::time::Instant::now();

    // 1. Рецепт: JSON формата GraphDto (контракт 05-IPC).
    let text = std::fs::read_to_string(recipe_path)
        .map_err(|e| format!("cannot read recipe '{recipe_path}': {e}"))?;
    let dto: GraphDto = serde_json::from_str(&text)
        .map_err(|e| format!("'{recipe_path}' is not a valid recipe file: {e}"))?;

    // 2. Валидация против реестра встроенных операций (UUID/DAG/версии)
    //    и загрузка графа в состояние исполнения.
    let registry = hexforge_ops::build_registry();
    let graph = validate_graph(dto, &registry).map_err(|e| e.message)?;
    let state = AppState::new(registry);
    {
        let mut g = state.graph.write();
        for node in graph.nodes.values() {
            g.insert_node(node.clone());
        }
    }

    // 3. Корни: узлы без входов. Поддержка N источников (multi-source).
    let mut roots: Vec<NodeId> = graph
        .nodes
        .values()
        .filter(|n| n.inputs.is_empty())
        .map(|n| n.id)
        .collect();
    if roots.is_empty() {
        return Err("recipe has no source nodes (nodes without inputs)".into());
    }
    // Детерминированный порядок корней — сортировка по UUID-строке, чтобы
    // `--in file1 --in file2` маппилось стабильно независимо от HashMap-порядка.
    roots.sort_by_key(|a| a.to_string());

    // Проверка соответствия числа --in и числа корней
    if in_paths.len() != 1 && in_paths.len() != roots.len() {
        return Err(format!(
            "number of --in files ({}) must be 1 or match number of source nodes ({})",
            in_paths.len(),
            roots.len()
        ));
    }

    // Создаём SourceEntry для каждого --in (mmap >16MiB, иначе InMemory)
    let handles: Vec<uuid::Uuid> = {
        let mut hs = Vec::with_capacity(if in_paths.len() == 1 { 1 } else { roots.len() });
        let paths_to_create: Vec<&String> = if in_paths.len() == 1 {
            vec![&in_paths[0]]
        } else {
            in_paths.iter().collect()
        };
        for in_path in paths_to_create {
            let file = std::fs::File::open(in_path)
                .map_err(|e| format!("cannot open input '{in_path}': {e}"))?;
            let meta = file
                .metadata()
                .map_err(|e| format!("cannot stat input '{in_path}': {e}"))?;
            let handle = if meta.len() > 16 * 1024 * 1024 {
                match unsafe { memmap2::Mmap::map(&file) } {
                    Ok(mmap) => {
                        let mut sources = state.sources.write();
                        sources.insert(SourceEntry::Mapped(mmap))
                    }
                    Err(_) => {
                        let bytes = std::fs::read(in_path)
                            .map_err(|e| format!("cannot read input '{in_path}': {e}"))?;
                        let mut sources = state.sources.write();
                        sources.insert(SourceEntry::InMemory(bytes))
                    }
                }
            } else if meta.len() == 0 {
                let mut sources = state.sources.write();
                sources.insert(SourceEntry::InMemory(Vec::new()))
            } else {
                let bytes = std::fs::read(in_path)
                    .map_err(|e| format!("cannot read input '{in_path}': {e}"))?;
                let mut sources = state.sources.write();
                sources.insert(SourceEntry::InMemory(bytes))
            };
            hs.push(handle);
        }
        hs
    };

    // Привязываем handles к корням: 1 handle → все корни, N handles → N корней по порядку
    {
        for (idx, root_id) in roots.iter().enumerate() {
            let handle = if handles.len() == 1 {
                handles[0]
            } else {
                handles[idx]
            };
            let existing = { state.graph.read().nodes.get(root_id).cloned() };
            if let Some(mut node) = existing {
                // Сохраняем собственные params корня (напр. alphabet у
                // base64.decode) — добавляем только sourceHandle, как и
                // GUI через bindSourceHandle (src/lib/graphMutate.ts).
                if node.params.is_object() {
                    node.params["sourceHandle"] = json!(handle.to_string());
                } else {
                    node.params = json!({ "sourceHandle": handle.to_string() });
                }
                state.graph.write().insert_node(node);
            }
        }
    }

    // 4. Стоки: узлы, не потребляемые никем. Ровно один — результат рецепта.
    let consumed: HashSet<NodeId> = graph
        .nodes
        .values()
        .flat_map(|n| n.inputs.iter().copied())
        .collect();
    let sinks: Vec<NodeId> = graph
        .nodes
        .keys()
        .copied()
        .filter(|id| !consumed.contains(id))
        .collect();
    if sinks.len() != 1 {
        return Err(format!(
            "recipe must have exactly one output node, found {}: {:?}",
            sinks.len(),
            sinks.iter().map(|id| id.to_string()).collect::<Vec<_>>()
        ));
    }
    let sink = sinks[0];

    // 5. Исполнение через общий с GUI планировщик; прогресс — в stderr.
    let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let output = hexforge_engine::scheduler::execute_chain(&state, &sink, &token, &|event| {
        eprintln!("[progress] {} {}B", event.node_id, event.bytes_processed);
    })
    .map_err(|e| format!("{:?}: {}", e.kind, e.message))?;

    // 6. Запись результата.
    std::fs::write(out_path, output.as_slice())
        .map_err(|e| format!("cannot write output '{out_path}': {e}"))?;

    let executed_nodes = state.history.read().order.len();

    Ok(RunSummary {
        output_bytes: output.len() as u64,
        duration_ms: started.elapsed().as_millis() as u64,
        executed_nodes,
    })
}
/// Валидация рецепта без запуска: проверяет JSON, UUID, DAG, наличие
/// операций в реестре и соответствие версий. Для CI-пайплайнов (FR-7.3).
pub fn validate_recipe(recipe_path: &str) -> Result<String, String> {
    validate_cli_path(recipe_path, "recipe")?;
    let text = std::fs::read_to_string(recipe_path)
        .map_err(|e| format!("cannot read recipe '{recipe_path}': {e}"))?;
    let dto: hexforge_engine::graph_dto::GraphDto = serde_json::from_str(&text)
        .map_err(|e| format!("'{recipe_path}' is not a valid recipe file: {e}"))?;

    let registry = hexforge_ops::build_registry();
    let graph =
        hexforge_engine::graph_dto::validate_graph(dto, &registry).map_err(|e| e.message)?;

    Ok(format!(
        "recipe valid: {} node(s), {} operation(s) in registry",
        graph.nodes.len(),
        registry.len()
    ))
}

/// Plugin SDK: генерация ключевой пары разработчика.
///
/// Возвращает `(pubkey_hex, signing_key_hex)`. Публичный ключ прикладывается
/// к install-запросу (TOFU), секретный подписывает `manifest.json`.
pub fn plugin_keygen() -> (String, String) {
    hexforge_plugin_host::generate_keypair()
}

/// Plugin SDK: подпись байтов `manifest.json`.
///
/// Возвращает hex-подпись для install-запроса. Подписываются ровно те байты,
/// что поедут на установку: любое переформатирование после подписи ломает её.
pub fn plugin_sign_manifest(manifest_path: &str, signing_key_hex: &str) -> Result<String, String> {
    validate_cli_path(manifest_path, "manifest")?;
    if signing_key_hex.trim().is_empty() {
        return Err("signing key must not be empty (see `plugin keygen`)".into());
    }
    let bytes = std::fs::read(manifest_path)
        .map_err(|e| format!("cannot read manifest '{manifest_path}': {e}"))?;
    hexforge_plugin_host::sign_manifest(&bytes, signing_key_hex)
        .map_err(|e| format!("cannot sign manifest '{manifest_path}': {e}"))
}

/// Plugin SDK: проверка `manifest.json` (JSON-форма + семантика полей).
/// Для CI: падает до подписи/установки с понятной ошибкой.
pub fn plugin_validate_manifest(manifest_path: &str) -> Result<String, String> {
    validate_cli_path(manifest_path, "manifest")?;
    let bytes = std::fs::read(manifest_path)
        .map_err(|e| format!("cannot read manifest '{manifest_path}': {e}"))?;
    let manifest: hexforge_plugin_host::PluginManifest = serde_json::from_slice(&bytes)
        .map_err(|e| format!("'{manifest_path}' is not a valid manifest file: {e}"))?;
    hexforge_plugin_host::validate_manifest(&manifest)
        .map_err(|e| format!("'{manifest_path}': {e}"))?;
    Ok(format!(
        "manifest valid: id={} version={}",
        manifest.id, manifest.version
    ))
}

/// Plugin SDK: binds `plugin.wasm` to `manifest.json` BEFORE signing.
///
/// Writes the artifact's SHA-256 into the manifest's `wasm_sha256` field
/// (file rewritten in place, pretty JSON). Sign the resulting bytes with
/// `plugin sign`: install and every later discovery re-verify the binding,
/// so a post-install `.wasm` swap fails closed instead of going unnoticed.
pub fn plugin_bind_artifact(manifest_path: &str, wasm_path: &str) -> Result<String, String> {
    validate_cli_path(manifest_path, "manifest")?;
    validate_cli_path(wasm_path, "wasm")?;
    let manifest_bytes = std::fs::read(manifest_path)
        .map_err(|e| format!("cannot read manifest '{manifest_path}': {e}"))?;
    let wasm_bytes =
        std::fs::read(wasm_path).map_err(|e| format!("cannot read wasm '{wasm_path}': {e}"))?;
    let bound = hexforge_plugin_host::bind_wasm_artifact(&manifest_bytes, &wasm_bytes)
        .map_err(|e| format!("cannot bind '{wasm_path}' into '{manifest_path}': {e}"))?;
    std::fs::write(manifest_path, &bound)
        .map_err(|e| format!("cannot write bound manifest '{manifest_path}': {e}"))?;
    let manifest: hexforge_plugin_host::PluginManifest = serde_json::from_slice(&bound)
        .map_err(|e| format!("bound manifest failed to parse (bug): {e}"))?;
    Ok(format!(
        "bound: id={} wasm_sha256={}",
        manifest.id,
        manifest.wasm_sha256.unwrap_or_default()
    ))
}

/// Official template sources (single source of truth: `plugins/example-wit`).
/// `plugin new` copies these verbatim, except `manifest.json`, which is
/// re-issued for the new plugin (id/name/version reset, no stale binding).
const TEMPLATE_CARGO_TOML: &str = include_str!("../../../plugins/example-wit/Cargo.toml");
const TEMPLATE_LIB_RS: &str = include_str!("../../../plugins/example-wit/src/lib.rs");
const TEMPLATE_WIT: &str = include_str!("../../../plugins/example-wit/wit/plugin.wit");
const TEMPLATE_MANIFEST_JSON: &str = include_str!("../../../plugins/example-wit/manifest.json");
const TEMPLATE_README_MD: &str = include_str!("../../../plugins/example-wit/README.md");

/// Plugin SDK: scaffold a new plugin from the official template.
///
/// Creates `<dir>/` with `Cargo.toml`, `src/lib.rs`, `wit/plugin.wit`,
/// `manifest.json` (fresh id/name/version `0.1.0`, author kept from the
/// template for you to edit, `wasm_sha256` removed — run `plugin bind`
/// after building) and `README.md` (the developer guide). Refuses to touch
/// a non-empty directory: scaffolding must never silently overwrite code.
pub fn plugin_new(dir: &str, id: Option<&str>, name: Option<&str>) -> Result<String, String> {
    validate_cli_path(dir, "dir")?;
    let root = std::path::Path::new(dir);
    if root.exists() {
        let non_empty = std::fs::read_dir(root)
            .map_err(|e| format!("cannot list directory '{dir}': {e}"))?
            .next()
            .is_some();
        if non_empty {
            return Err(format!(
                "refusing to scaffold into non-empty directory '{dir}'"
            ));
        }
    } else {
        std::fs::create_dir_all(root)
            .map_err(|e| format!("cannot create directory '{dir}': {e}"))?;
    }
    let default_id = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "my-plugin".into());
    let id = id.unwrap_or(&default_id);
    let name = name.unwrap_or(id);

    let mut manifest_value: serde_json::Value = serde_json::from_str(TEMPLATE_MANIFEST_JSON)
        .map_err(|e| format!("template manifest is corrupt (bug): {e}"))?;
    let obj = manifest_value
        .as_object_mut()
        .ok_or("template manifest is not a JSON object (bug)")?;
    obj.insert("id".into(), serde_json::Value::String(id.to_string()));
    obj.insert("name".into(), serde_json::Value::String(name.to_string()));
    obj.insert("version".into(), serde_json::Value::String("0.1.0".into()));
    obj.remove("wasm_sha256");
    obj.insert(
        "granted_capabilities".into(),
        serde_json::Value::Array(Vec::new()),
    );
    let manifest: hexforge_plugin_host::PluginManifest =
        serde_json::from_value(manifest_value.clone())
            .map_err(|e| format!("scaffolded manifest invalid (bug): {e}"))?;
    hexforge_plugin_host::validate_manifest(&manifest)
        .map_err(|e| format!("scaffolded manifest invalid (bug): {e}"))?;

    for (rel, content) in [
        ("Cargo.toml", TEMPLATE_CARGO_TOML),
        ("src/lib.rs", TEMPLATE_LIB_RS),
        ("wit/plugin.wit", TEMPLATE_WIT),
        ("README.md", TEMPLATE_README_MD),
    ] {
        let dest = root.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create '{}': {e}", parent.display()))?;
        }
        std::fs::write(&dest, content)
            .map_err(|e| format!("cannot write '{}': {e}", dest.display()))?;
    }
    let manifest_text = serde_json::to_string_pretty(&manifest_value)
        .map_err(|e| format!("cannot render manifest (bug): {e}"))?;
    std::fs::write(root.join("manifest.json"), manifest_text + "\n")
        .map_err(|e| format!("cannot write manifest.json: {e}"))?;

    Ok(format!(
        "created plugin '{id}' in '{dir}'\nnext: edit src/lib.rs + manifest.json (author), then\n  cargo build --release --target wasm32-wasip1\n  wasm-tools component new target/wasm32-wasip1/release/<crate>.wasm -o plugin.wasm\n  hexforge-cli plugin validate manifest.json\n  hexforge-cli plugin bind manifest.json plugin.wasm\n  hexforge-cli plugin sign manifest.json --key <hex>\n  hexforge-cli plugin install plugin.wasm manifest.json --root <library> --sig <hex> --pub <hex>"
    ))
}

/// Plugin SDK: headless install into a plugin library root.
///
/// Same backend as the Tauri `install_plugin` command
/// (`PluginLibrary::install_package`: signature → manifest → binding →
/// WIT-contract → capabilities, staged + atomic). Signature/pubkey come
/// from `--sig`/`--pub` or, when omitted, from the `<manifest>.sig` /
/// `<manifest>.pub` sidecars. `--root` is required: installs never write
/// to an implicit location.
pub fn plugin_install(
    wasm_path: &str,
    manifest_path: &str,
    root: &str,
    sig: Option<&str>,
    pubkey: Option<&str>,
) -> Result<String, String> {
    validate_cli_path(wasm_path, "wasm")?;
    validate_cli_path(manifest_path, "manifest")?;
    validate_cli_path(root, "root")?;
    let wasm_bytes =
        std::fs::read(wasm_path).map_err(|e| format!("cannot read wasm '{wasm_path}': {e}"))?;
    let manifest_bytes = std::fs::read(manifest_path)
        .map_err(|e| format!("cannot read manifest '{manifest_path}': {e}"))?;
    let read_sidecar = |suffix: &str, flag: Option<&str>, what: &str| -> Result<String, String> {
        if let Some(v) = flag {
            if v.trim().is_empty() {
                return Err(format!("{what} must not be empty"));
            }
            return Ok(v.trim().to_string());
        }
        let path = format!("{manifest_path}{suffix}");
        std::fs::read_to_string(&path).map_err(|_| {
            format!("missing {what}: pass --{what} <hex> or write the '{path}' sidecar")
        })
    };
    let sig = read_sidecar(".sig", sig, "sig")?;
    let pubkey = read_sidecar(".pub", pubkey, "pub")?;

    let library =
        hexforge_plugin_host::store::PluginLibrary::new(std::path::PathBuf::from(root), Vec::new());
    let found = library
        .install_package(&wasm_bytes, &manifest_bytes, sig.trim(), pubkey.trim())
        .map_err(|e| format!("install refused: {e}"))?;
    let stored = found
        .manifest
        .ok_or("installed package has no manifest (bug)")?;
    Ok(format!(
        "installed: id={} version={} status={:?}",
        stored.id, stored.version, found.status
    ))
}

/// Shared grant/revoke core: same backend as the Tauri commands
/// (`PluginLibrary::get` for fresh state + `set_grants` for the
/// re-verified, policy-clamped persistent write). No separate trust model.
fn plugin_set_grant(
    verb: &str,
    plugin_id: &str,
    root: &str,
    capability: &str,
) -> Result<String, String> {
    if plugin_id.trim().is_empty() {
        return Err("plugin id must not be empty".into());
    }
    validate_cli_path(root, "root")?;
    if capability.trim().is_empty() {
        return Err("capability must not be empty".into());
    }
    if !hexforge_plugin_host::store::POLICY_CAPABILITIES.contains(&capability) {
        return Err(format!(
            "unknown capability '{capability}' (policy allows: {:?})",
            hexforge_plugin_host::store::POLICY_CAPABILITIES
        ));
    }
    let library =
        hexforge_plugin_host::store::PluginLibrary::new(std::path::PathBuf::from(root), Vec::new());
    let entry = library.get(plugin_id).ok_or_else(|| {
        format!("unknown plugin '{plugin_id}' (only installed packages accept persisted grants)")
    })?;
    let manifest = entry
        .manifest
        .ok_or_else(|| format!("plugin '{plugin_id}' has no readable manifest; reinstall it"))?;
    let mut next = manifest.granted_capabilities;
    if verb == "grant" {
        if !next.iter().any(|c| c == capability) {
            next.push(capability.to_string());
        }
    } else {
        next.retain(|c| c != capability);
    }
    let effective = library
        .set_grants(plugin_id, &next)
        .map_err(|e| format!("{verb} refused: {e}"))?;
    let past = if verb == "grant" {
        "granted"
    } else {
        "revoked"
    };
    Ok(format!(
        "{past}: id={plugin_id} capability={capability} effective_grants={effective:?}"
    ))
}

/// Plugin SDK: persist a capability grant (headless twin of the UI flow).
pub fn plugin_grant(plugin_id: &str, root: &str, capability: &str) -> Result<String, String> {
    plugin_set_grant("grant", plugin_id, root, capability)
}

/// Plugin SDK: persist a capability revocation (headless twin of the UI flow).
pub fn plugin_revoke(plugin_id: &str, root: &str, capability: &str) -> Result<String, String> {
    plugin_set_grant("revoke", plugin_id, root, capability)
}

/// Plugin SDK: execute an installed, verified plugin.
///
/// Fresh discovery on every call (same as the app startup path), so grants
/// or artifacts changed since install are honored or refused exactly as the
/// UI would. Input comes from `--in`, output goes to `--out`; execution
/// failures (denied capability, trap, fuel exhaustion) propagate verbatim.
pub fn plugin_run(
    plugin_id: &str,
    root: &str,
    input_path: &str,
    output_path: &str,
) -> Result<String, String> {
    if plugin_id.trim().is_empty() {
        return Err("plugin id must not be empty".into());
    }
    validate_cli_path(root, "root")?;
    validate_cli_path(input_path, "input")?;
    validate_cli_path(output_path, "output")?;

    let library =
        hexforge_plugin_host::store::PluginLibrary::new(std::path::PathBuf::from(root), Vec::new());
    let entry = library
        .get(plugin_id)
        .ok_or_else(|| format!("unknown plugin '{plugin_id}' (is it installed under '{root}'?)"))?;
    if entry.status != hexforge_plugin_host::store::PluginStatus::Verified {
        return Err(format!(
            "plugin '{plugin_id}' is not runnable ({:?}): {}",
            entry.status,
            entry.error.unwrap_or_else(|| "unknown reason".into())
        ));
    }
    let instance = entry
        .instance
        .ok_or_else(|| format!("plugin '{plugin_id}' has no executable instance; reinstall it"))?;
    // Identity resolved before touching input: a wrong id reports as such
    // even when the input path is also bad.
    let input =
        std::fs::read(input_path).map_err(|e| format!("cannot read input '{input_path}': {e}"))?;
    let runtime = hexforge_plugin_host::PluginRuntime::new(None)
        .map_err(|e| format!("cannot start plugin runtime: {e}"))?;
    let output = runtime
        .execute(&instance, &input)
        .map_err(|e| format!("run failed: {e}"))?;
    std::fs::write(output_path, &output)
        .map_err(|e| format!("cannot write output '{output_path}': {e}"))?;
    Ok(format!(
        "ran: id={plugin_id} in_bytes={} out_bytes={} wrote={output_path}",
        input.len(),
        output.len()
    ))
}

/// Plugin SDK: list installed plugins with verification status (headless
/// twin of the Tauri `list_plugins` command: same `discover()` backend,
/// same fail-closed statuses). Sorted by id for stable script output.
pub fn plugin_list(root: &str) -> Result<String, String> {
    validate_cli_path(root, "root")?;
    let library =
        hexforge_plugin_host::store::PluginLibrary::new(std::path::PathBuf::from(root), Vec::new());
    let mut entries = library.discover();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    if entries.is_empty() {
        return Ok(format!("no plugins installed under '{root}'"));
    }
    let lines: Vec<String> = entries
        .iter()
        .map(|entry| {
            let (version, requested, granted) = match &entry.manifest {
                Some(manifest) => (
                    manifest.version.clone(),
                    manifest.requested_capabilities.join(","),
                    manifest.granted_capabilities.join(","),
                ),
                None => ("?".into(), String::new(), String::new()),
            };
            let mut line = format!(
                "{id} version={version} status={status:?} requested=[{requested}] granted=[{granted}]",
                id = entry.id,
                status = entry.status,
            );
            if let Some(reason) = &entry.error {
                line.push_str(&format!(" error={reason}"));
            }
            line
        })
        .collect();
    Ok(lines.join("\n"))
}
