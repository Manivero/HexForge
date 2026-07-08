# HexForge — Information Architecture & Component Tree

## 1. Верхнеуровневая карта экрана

```
┌──────────────────────────────────────────────────────────────────────┐
│ TitleBar (native, frameless, custom controls)                        │
├───────────┬────────────────────────────────────────┬─────────────────┤
│           │                                        │                 │
│ ActivityBar│           GraphCanvas                │  InspectorPanel │
│ (иконки:   │   (нелинейный DAG узлов, pan/zoom)    │  (параметры     │
│  Graph,    │                                        │   выбранного    │
│  History,  │                                        │   узла / вывод) │
│  Plugins,  │                                        │                 │
│  Files)    │                                        │                 │
│           ├────────────────────────────────────────┤                 │
│           │        PreviewDock (hex/text/image)     │                 │
├───────────┴────────────────────────────────────────┴─────────────────┤
│ StatusBar (позиция, размер файла, throughput, encoding detect)        │
└──────────────────────────────────────────────────────────────────────┘

           CommandPalette — модальный оверлей поверх всего (⌘K)
```

## 2. Дерево компонентов (React)

```
<App>                                                  state: theme, activeWorkspaceId
├── <TitleBar/>                                        props: { platform }
├── <WorkspaceProvider>                                 zustand store boundary
│   ├── <ActivityBar>                                  state: activeView
│   │   └── <ActivityBarItem/> × N                      props: { icon, view, isActive, onSelect }
│   │
│   ├── <GraphCanvas>                                   state (store): nodes, edges, viewport, selectedNodeId
│   │   ├── <GraphViewport>                             props: { viewport, onPan, onZoom }
│   │   │   ├── <GraphEdgeLayer/>                        props: { edges, nodesById }
│   │   │   └── <OperationNode/> × N                     props: { node, isSelected, onSelect, onConnectStart, onConnectEnd }
│   │   │       ├── <NodeHeader/>                        props: { title, opCategory, status: 'idle'|'running'|'error'|'stale' }
│   │   │       ├── <NodePorts/>                         props: { inputs, outputs, onPortDragStart }
│   │   │       └── <NodeInlineSummary/>                 props: { outputPreviewBytes, truncated }
│   │   ├── <GraphMinimap/>                              props: { nodes, viewport }
│   │   └── <GraphContextMenu/>                          props: { position, targetNodeId, onAction }
│   │
│   ├── <InspectorPanel>                                 state (derived): selectedNode
│   │   ├── <OperationParamsForm/>                       props: { schema: JSONSchema, values, onChange }
│   │   │   └── <ParamField/> × N                        props: { field, value, onChange } (variants: text/number/select/bytes/enum)
│   │   ├── <OperationMeta/>                              props: { deterministic, streamable, memoryCost }
│   │   └── <HistoryTimeline/>                            props: { snapshots: StateNode[], currentId, onJumpTo, onBranchFrom }
│   │
│   ├── <PreviewDock>                                     state: activeTab ('hex'|'text'|'image'|'diff')
│   │   ├── <HexViewer/>                                  props: { source: ByteSourceRef, cursor, onCursorMove } — virtualized
│   │   ├── <TextPreview/>                                props: { source: ByteSourceRef, encoding }
│   │   ├── <ImagePreview/>                               props: { source: ByteSourceRef, mime }
│   │   └── <DiffView/>                                   props: { left: ByteSourceRef, right: ByteSourceRef }
│   │
│   └── <StatusBar/>                                      props: { fileSize, throughputBps, cursorOffset, detectedEncoding }
│
├── <CommandPalette>                                      state: query, filteredResults, selectedIndex
│   ├── <CommandInput/>                                   props: { value, onChange, placeholder }
│   ├── <CommandResultsList/>                              props: { groups: CommandGroup[], selectedIndex }
│   │   └── <CommandResultItem/> × N                       props: { item, isSelected, onSelect }
│   └── <CommandEmptyState/>
│
├── <PluginManagerModal>                                   state: installedPlugins, pendingGrants
│   ├── <PluginList/>                                       props: { plugins }
│   ├── <PluginDetail/>                                     props: { plugin, onRevokeCapability }
│   └── <CapabilityGrantDialog/>                            props: { requestedCapabilities, onApprove, onDeny }
│
└── <ToastLayer/>                                           props: { toasts } — прогресс длительных операций, ошибки
```

## 3. Владение состоянием (Zustand slices)

| Slice | Ответственность | Ключевые поля |
|---|---|---|
| `graphSlice` | Топология DAG | `nodes: Record<NodeId, OperationNode>`, `edges: Edge[]`, `selectedNodeId` |
| `historySlice` | Time-travel state graph | `snapshots: Record<SnapshotId, Snapshot>`, `currentSnapshotId`, `branches` |
| `dataSlice` | Ссылки на байтовые источники (НЕ сами байты — только handle в Rust-стороне) | `sources: Record<SourceHandle, SourceMeta>` |
| `paletteSlice` | Состояние Command Palette | `isOpen`, `query`, `recentCommandIds` |
| `pluginSlice` | Установленные плагины и capability grants | `plugins: Record<PluginId, PluginManifest>`, `grants` |
| `uiSlice` | Чисто визуальное состояние, не подлежит персистентности в recipe | `theme`, `activeView`, `panelSizes` |

Правило разделения: любое состояние, которое должно попасть в экспортируемый
`.hexforge`-файл (граф, снапшоты, параметры операций), живёт в
`graphSlice`/`historySlice`. Всё остальное — эфемерное UI-состояние
(`uiSlice`, `paletteSlice`) и в экспорт не попадает.

## 4. Правило владения данными (важно для Node Graph)

Байты никогда не хранятся в React/Zustand напрямую. Frontend хранит только
`SourceHandle` (opaque id, выданный Rust-ядром при загрузке файла или создании
промежуточного результата). Превью запрашивается точечно — команда
`preview_bytes(handle, offset, length)` — так UI никогда не тянет полный
буфер в JS heap. Это прямое следствие NFR-2 (32 ГБ файлы) и предотвращает
ситуацию CyberChef, где вывод операции дублируется в DOM/JS-состоянии.
