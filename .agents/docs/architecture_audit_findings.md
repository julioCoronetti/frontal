# Frontal Architecture Audit Findings & Follow-Up Tracker (Parte 2)

**Documento complementar de:** [`architecture_roadmap.md`](./architecture_roadmap.md)  
**Última atualização:** Fase 2 auditada e consolidada na `develop` (sobre `9fc79a0`; `quality_gate.sh --full`: 15 unitários + 326 testes de integração aprovados).  
**Finalidade:** Registrar apontamentos abertos, débitos parciais e riscos verificados por fase para impedir que itens críticos sejam esquecidos ao avançar para as próximas etapas.

---

## 1. Painel de Notas por Fase

| Escopo | Nota | Estado | Resumo Objetivo |
| :--- | :---: | :--- | :--- |
| **Fase 0** (Blindagem & Scale Baseline) | **9.2 / 10** | Endurecida na Fase 2 | `boundaries.rs` fechou aliases de `Mutex`/`RwLock`, `CoreHardcodedStoragePath` e `UiWildcardInfraReexport` (`LEGACY_ALLOWLIST` caiu de 16 para 1 entrada); resta apenas trabalho real em `scale_baseline.rs` (Fase 4). |
| **Fase 1** (Ciclo de Vida & Estado Único) | **9.4 / 10** | Consolidada na Fase 2 | `ProcessGroupGuard` migrado para thread OS reaper dedicada, `#[cfg(not(unix))]` adicionado em `usage/cli.rs`, `self.sessions`/`deleted_ids` e abertura síncrona removidos de `PromptHistory`; restam conexões read-only avulsas e `started_at` não lido. |
| **Fase 2** (Event Loop & Separação de Camadas) | **9.1 / 10** | Concluída (pronta p/ Fase 3) | Event loop 100% não bloqueante, `OpToken` em respawns/clipboard, limpeza de `dormant_metadata`, remoção de `Mutex` na UI e no `interrupt`, startup/histórico sem bloqueio, e gaps residuais `MEDIUM`/`LOW` da auditoria fechados. |
| **Média das Entregas (Fases 0–2)** | **9.2 / 10** | — | Arquitetura geral evoluiu de **5.5/10** -> **7.0/10** -> **9.2/10** (0 `CRITICAL`/`HIGH`; critério do roadmap >= 9.0 atingido, pronta para iniciar a Fase 3). |

---

## 2. Apontamentos — Fase 0

- [x] **Cobertura adicional em `tests/domains/architecture/boundaries.rs`** *(resolvida na Fase 2)*:
  - `has_static_mutex_or_rwlock` (`boundaries.rs:148-173`) agora resolve aliases `type` e `use ... as ...` transitivos (`collect_lock_type_aliases`).
  - `scan_core_violations` (`boundaries.rs:258-295`) bloqueia `CoreHardcodedStoragePath` (`opencode.db`, `history.db`, `conversation_summaries.db`, `antigravity-oauth-token`, `transcript.jsonl`).
  - `scan_ui_violations` (`boundaries.rs:308-347`) bloqueia todos os paths de storage acima e `UiWildcardInfraReexport` (`pub use crate::infra::...*`).
  - `LEGACY_ALLOWLIST` reduzida de 16 para **1 entrada** (`("src/ui/app/respawn.rs", BoundaryRule::UiSesPrefixHeuristic)` — escopo da Fase 3).
- [ ] **Métricas reais de trabalho em `tests/domains/cache_and_markdown/scale_baseline.rs`** *(cobrar na Fase 4)*:
  - Os 4 testes verificam apenas `cache_misses() <= 1` e contagem total de linhas, passando mesmo com re-parse $O(M^2)$ no streaming e scans $O(B)$ em `visible_slice` / `has_unconsolidated_blocks`.
  - Adicionar contadores/asserções para: número de invocações ao parser Markdown por burst de deltas (parse coalescido 60 Hz), tempo/operações em `visible_slice` para sessão grande ($O(\log B)$) e teto de entradas retidas no `l2_cache` após sequência de resizes.

---

## 3. Histórico & Residuais — Fase 1

### 3.1 Máquina de Estados `TurnState` (`src/ui/tab.rs`)
- [x] Eliminar `waiting_for_response` paralelo e derivar `is_waiting_for_response()` de `TurnState::Submitting`.
- [x] Fechar desvios de `transition_turn` em `stream.rs`, `clear_active_tab`, `close_or_reset_tab` e `cli.rs`, além de tratar `TurnState::Interrupting` em `StreamActivity` e limpar `in_flight_usage` em `TurnCompleted`.
- [ ] *(Menor)* `TurnState::Streaming { started_at }` (`src/ui/tab.rs`) ainda não é lido externamente (a UI lê `tab.turn_started_at`).

### 3.2 Centralização SQLite & Split-Brain de Histórico (`src/infra/history/`)
- [x] Remover `agent_cache: Arc<RwLock<Vec<SessionRecord>>>` da UI (`PromptHistory`), mover para o worker e conectar `HistorySnapshot` (`sessions` + `agent_sessions`) em `resume.rs`, `respawn.rs` e `sessions.rs`.
- [x] Aplicar `PRAGMA synchronous = NORMAL;` no `bg_conn` (`worker.rs`), unificar precedência de títulos (`>=` via `TitleSource::can_override`) e checar `cid` normalizado em `sync.rs`.
- [x] Eliminar `self.sessions` / `self.deleted_ids` (`publish_local_snapshot`) do lado da UI em `PromptHistory` (`src/infra/history/mod.rs:136-148`, isolando `InMemoryStore` só em `open_in_memory`), mover abertura/migração SQLite de `open_with_external_dbs` para `spawn_history_worker` (`worker.rs:21-102`), conectar hidratação de respawn via `HydrationClient` (`&bg_conn`) e remover `pub use crate::infra::history::*;` de `src/ui/history.rs:4`.
- [ ] *(Residual / consolidar antes ou na Fase 3)* Consolidar aberturas SQLite ad-hoc restantes em `src/infra/history/` (`hydrate.rs:120-128` fallback, `messages.rs:22,47,69,90-100`, `telemetry.rs:53-62`, `agent.rs:75,110,134,155,173` e `opencode/mod.rs:391-398`).

### 3.3 `ProcessGroupGuard`, `SessionConfig`, `reconfigure` e Docs
- [x] Eliminar todas as chamadas diretas a `libc::killpg` fora de `src/infra/engine/process.rs` e sinalizar `SIGTERM` na saída natural de `opencode.rs` e `clipboard/platform.rs`.
- [x] Corrigir `AcpEngine::reconfigure` (`shutting_down = true` apenas após `Self::spawn` bem-sucedido).
- [x] Eliminar duplicação dos 11 campos entre `SessionConfig` e `EngineConfig` (`SessionConfig(pub EngineConfig)`), usar `default_history_path()` em `App::new` e sincronizar `AGENTS.md` e `README.md`.
- [x] Endurecer `ProcessGroupGuard::drop` (`src/infra/engine/process.rs:63-148`) com thread OS dedicada `"frontal-pgid-reaper"` + `BinaryHeap` de deadlines (independente do runtime Tokio, com fallback imediato para `SIGKILL` só se falhar envio/spawn) e adicionar `#[cfg(not(unix))]` kill explícito no timeout de `src/infra/usage/cli.rs:24-83`.
- [x] *(Corrigido no fechamento da Fase 2)* Atualizar `.agents/docs/structure.md` e `.wiki/Frontal-Architecture.md` (ver item 4.4).

---

## 4. Apontamentos Abertos — Fase 2 (Corrigir Antes de Fechar a Fase)

### 4.1 O Que Já Está Entregue na Fase 2
- [x] `UserConfig` (`src/core/config.rs`), `ModelCatalog` (`src/core/model_catalog.rs`) e `SessionId::new()` (`src/core/protocol/mod.rs`) puros sem I/O (`std::fs` e `rusqlite` extraídos para `src/infra/config_store.rs` e `src/infra/model_catalog.rs`).
- [x] `UsageService` instanciado em `src/infra/usage/` (`mod.rs`, `cli.rs`, `fetch.rs`, `opencode_api.rs`, `stats.rs`) + tipos/parsers puros em `src/core/usage/`, removendo `USAGE_CACHE`, `OPENCODE_USAGE_CACHE`, `OPENCODE_STATS_CACHE` e todo I/O/subprocesso/SQLite de `src/ui/modal/usage/`.
- [x] `FileSearchIndex` (`src/infra/fs_search.rs`) migrado de `RwLock` para `tokio::sync::watch::Receiver`.
- [x] Todos os 9 braços do `tokio::select!` (`src/ui/mod.rs:77-286`) livres de `.await` inline para `spawn`, `shutdown`, `respawn`, `hydrate`, `send_prompt`, `CopyClipboard` (`arboard` via `spawn_blocking`) e `PasteClipboard`; `AcpEngine::send_prompt` + `prep_worker` (`src/infra/engine/acp/coordinator.rs:423-453`) executando leitura de mídia e Base64 em `spawn_blocking` com `cancel_gen`.

### 4.2 Bugs de Concorrência & Cancelamento (`OpToken`) — Corrigidos
- [x] **Corrigir descarte obsoleto de `RespawnFinished` em `src/ui/mod.rs` (`handle_bg_event`):**
  - `apply_respawn_outcome` (`src/ui/app/respawn.rs`) agora retorna `Result<Option<SessionId>, EngineError>` (`Ok(None)` para descarte por mismatch de `OpToken`), e `handle_bg_event` valida `(tab_id, generation_id)` tanto em `Ok(Some(_))` quanto em `Err(_)`, ignorando silenciosamente `Ok(None)`.
- [x] **Corrigir vazamento de `dormant_metadata` em `src/ui/app/respawn.rs`:**
  - No caminho de descarte de `apply_respawn_outcome`, quando `plan.shares_active_conv` é `true` e nenhuma aba referencia `plan.old_id`, `session_manager.remove_dormant_metadata(&plan.old_id)` é executado (coberto por teste unitário em `src/ui/app/respawn_tests.rs`).
- [x] **Proteger `PasteClipboard` / `InspectPastedText` com `OpToken` (`src/ui/mod.rs`, `src/ui/app/events/mod.rs`):**
  - Canal `paste_tx`/`paste_rx` e `App::apply_paste_outcome` agora carregam e validam `OpToken { tab_id, generation_id }` via `App::active_op_token()`.

### 4.3 Bloqueios e Locks Síncronos Residuais na UI / Startup — Corrigidos
- [x] **Remover `stdout().lock().flush()` síncrono ad-hoc do loop da UI:**
  - Notificações sensoriais (`src/ui/app/stream.rs` -> `notify::sensory_notification_bytes`) e sequências OSC 52 (`src/ui/mod.rs` -> `take_pending_osc52_bytes`) agora são enfileiradas em `App::pending_terminal_writes` e descarregadas de forma sincronizada com `Terminal::draw` via `TerminalGuard::write_raw_sequences` (`src/ui/terminal.rs`).
- [x] **Remover `std::sync::Mutex` síncrono de `UsageModal` e `OpenCodeEngine::interrupt`:**
  - `UsageModal` (`src/ui/modal/usage/mod.rs`, `render.rs`) migrado para estado UI-owned com `RefCell<UsageState>` / `RefCell<StatsState>` e `RefCell<Option<Receiver<...>>>` alimentado por `mpsc::try_recv`, sem `Arc`/`std::sync::Mutex`.
  - `OpenCodeEngine::interrupt` (`src/infra/engine/opencode.rs`) migrado para leitura atômica de `active_pgid` + `process::signal_pgid` (`src/infra/engine/process.rs`), sem adquirir `pgid_guard.lock()`.
- [x] **Eliminar I/O / bloqueio síncrono restante em startup e `Up` do histórico:**
  - `cli::run_from_env` carrega `config_store::load()` uma única vez via `tokio::task::spawn_blocking` e injeta o `UserConfig` em `EngineConfig::from_user_config` e `App::new_with_config`.
  - `PromptHistory::ensure_entries_loaded` (`src/infra/history/mod.rs`) removeu o fallback bloqueante `done_rx.recv_timeout(2000ms)` em `KeyCode::Up`, consultando apenas `entries_rx.borrow()` de forma não bloqueante.

### 4.4 Higiene de Commits e Paridade Documental — Concluída
- [x] **Fatiar os commits da Fase 2 respeitando o limite atômico:**
  - Entregas da Fase 2 fatiadas em commits lógicos (`6ba9be5`..`9fc79a0`) e ajustes de fechamento prontos para auditoria/commit.
- [x] **Sincronizar documentação com os módulos reais da Fase 2:**
  - [`.agents/docs/structure.md`](./structure.md), [`.wiki/Frontal-Architecture.md`](../../.wiki/Frontal-Architecture.md) e [`.agents/docs/architecture_roadmap.md`](./architecture_roadmap.md) atualizados com os módulos e responsabilidades reais da Fase 2.

---

## 5. Pontos de Atenção Pré-Mapeados — Fases 3 e 4

- **Fase 3 (`BackendAdapter`):** o backfill da coluna `backend` no SQLite (já iniciado na migração v1) deve ser verificado em todas as rotas de `/resume` e `external.rs` antes de apagar heurísticas `"ses_"` (18 ocorrências em 7 arquivos, incluindo a última entrada da `LEGACY_ALLOWLIST` em `src/ui/app/respawn.rs`) e `"/"`; aproveitar para remover campos mortos de `ModelEffortModal` (`models`, `ModelEffortStep::SelectEffort`) e padronizar `CommandRegistry`.
- **Fase 4 (Otimização 60 FPS):** validar o ganho diretamente no `scale_baseline.rs` evoluído (parse coalescido 1x/frame + prefix-sum $O(\log B)$ + `has_unconsolidated_blocks()` $O(1)$ + cache de layout no `PromptEditor` + eviction do `l2_cache`/highlighters/undo `paste_registry` + cota/hash forte no cache de mídia/PNG + extração clipboard via L1 com correção de coordenadas do rodapé/spinner).
