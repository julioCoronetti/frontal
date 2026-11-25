# Frontal Architecture Audit Findings & Follow-Up Tracker (Parte 2)

**Documento complementar de:** [`architecture_roadmap.md`](./architecture_roadmap.md)  
**Última atualização:** Fase 2 auditada no working tree da `develop` (sobre `dab0d52`; `ATOMIC_OVERRIDE=1 quality_gate.sh --full`: 14 unitários + 326 testes de integração aprovados).  
**Finalidade:** Registrar apontamentos abertos, débitos parciais e riscos verificados por fase para impedir que itens críticos sejam esquecidos ao avançar para as próximas etapas.

---

## 1. Painel de Notas por Fase

| Escopo | Nota | Estado | Resumo Objetivo |
| :--- | :---: | :--- | :--- |
| **Fase 0** (Blindagem & Scale Baseline) | **9.2 / 10** | Endurecida na Fase 2 | `boundaries.rs` fechou aliases de `Mutex`/`RwLock`, `CoreHardcodedStoragePath` e `UiWildcardInfraReexport` (`LEGACY_ALLOWLIST` caiu de 16 para 1 entrada); resta apenas trabalho real em `scale_baseline.rs` (Fase 4). |
| **Fase 1** (Ciclo de Vida & Estado Único) | **9.4 / 10** | Consolidada na Fase 2 | `ProcessGroupGuard` migrado para thread OS reaper dedicada, `#[cfg(not(unix))]` adicionado em `usage/cli.rs`, `self.sessions`/`deleted_ids` e abertura síncrona removidos de `PromptHistory`; restam conexões read-only avulsas e `started_at` não lido. |
| **Fase 2** (Event Loop & Separação de Camadas) | **8.3 / 10** | Em revisão (não commitada) | Extração de I/O e `tokio::select!` não bloqueante entregues, mas há bug de `generation_id` em `handle_bg_event`, leak de `dormant_metadata`, locks/flush síncronos residuais (`stdout`/OSC 52/`UsageModal`/`OpenCodeEngine::interrupt`), 56 arquivos sem commit e deriva documental. |
| **Média das Entregas (Fases 0–2)** | **9.0 / 10** | — | Arquitetura geral evoluiu de **5.5/10** -> **7.0/10** -> **8.0/10** (pronta para fechar pendências da Fase 2 e seguir à Fase 3). |

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
- [ ] *(Corrigir no fechamento da Fase 2)* Atualizar `.agents/docs/structure.md` e `.wiki/Frontal-Architecture.md` (ver item 4.4).

---

## 4. Apontamentos Abertos — Fase 2 (Corrigir Antes de Fechar a Fase)

### 4.1 O Que Já Está Entregue na Fase 2
- [x] `UserConfig` (`src/core/config.rs`), `ModelCatalog` (`src/core/model_catalog.rs`) e `SessionId::new()` (`src/core/protocol/mod.rs`) puros sem I/O (`std::fs` e `rusqlite` extraídos para `src/infra/config_store.rs` e `src/infra/model_catalog.rs`).
- [x] `UsageService` instanciado em `src/infra/usage/` (`mod.rs`, `cli.rs`, `fetch.rs`, `opencode_api.rs`, `stats.rs`) + tipos/parsers puros em `src/core/usage/`, removendo `USAGE_CACHE`, `OPENCODE_USAGE_CACHE`, `OPENCODE_STATS_CACHE` e todo I/O/subprocesso/SQLite de `src/ui/modal/usage/`.
- [x] `FileSearchIndex` (`src/infra/fs_search.rs`) migrado de `RwLock` para `tokio::sync::watch::Receiver`.
- [x] Todos os 9 braços do `tokio::select!` (`src/ui/mod.rs:77-286`) livres de `.await` inline para `spawn`, `shutdown`, `respawn`, `hydrate`, `send_prompt`, `CopyClipboard` (`arboard` via `spawn_blocking`) e `PasteClipboard`; `AcpEngine::send_prompt` + `prep_worker` (`src/infra/engine/acp/coordinator.rs:423-453`) executando leitura de mídia e Base64 em `spawn_blocking` com `cancel_gen`.

### 4.2 Bugs de Concorrência & Cancelamento (`OpToken`) — Corrigir Já
- [ ] **Corrigir descarte obsoleto de `RespawnFinished` em `src/ui/mod.rs:391-401` (`handle_bg_event`):**
  - Quando `apply_respawn_outcome` (`src/ui/app/respawn.rs:223-236`) detecta mismatch de `generation_id` e retorna `Err(EngineError::ProcessNotRunning)`, `handle_bg_event` ainda casa a aba apenas por `t.tab_id == tab_id` (`L393, L397`).
  - Em respawns consecutivos na mesma aba (ex.: `/model` ou `/resume` repetido antes do primeiro terminar), o retorno obsoleto dispara `handle_prompt_failure` ou sobrescreve `tab.status_text` na **geração mais nova**. Retornar resultado explícito de descarte obsoleto (ou validar `OpToken` no `Err`) e ignorar silenciosamente sem mutar a aba atual.
- [ ] **Corrigir vazamento de `dormant_metadata` em `src/ui/app/respawn.rs:223-236`:**
  - Quando `shares_active_conv` é `true` e a aba é fechada antes do retorno de `RespawnFinished`, `apply_respawn_outcome` retorna no mismatch sem remover `plan.old_id` de `session_manager.dormant_metadata`. Garantir limpeza de `dormant_metadata` também no caminho de descarte.
- [ ] **Proteger `PasteClipboard` / `InspectPastedText` com `OpToken` (`src/ui/mod.rs:129-152`, `src/ui/app/events/mod.rs:27-70`):**
  - Atualmente validam apenas `session_id`; após `reset_single_tab_empty` (que incrementa `generation_id` mantendo o mesmo `session_id`), um resultado em voo ainda é aplicado à aba resetada. Carregar `OpToken { tab_id, generation_id }` e validar ambos na aplicação.

### 4.3 Bloqueios e Locks Síncronos Residuais na UI / Startup — Corrigir ou Justificar
- [ ] **Remover `stdout().lock().flush()` síncrono do loop da UI:**
  - `flush_pending_osc52` (`src/ui/mod.rs:284, 420-429` -> `src/infra/clipboard/mod.rs:227-233`) e `emit_sensory_notification` (`src/ui/notify.rs:18-24` -> `src/ui/app/stream.rs:155-160`) ainda executam escrita/flush síncronos em `stdout` dentro do `tokio::select!`. Descarregar ou isolar fora do loop principal (garantindo sincronia com `ratatui` `Terminal::draw`).
- [ ] **Remover `std::sync::Mutex` síncrono de `UsageModal` e `OpenCodeEngine::interrupt`:**
  - `UsageModal` (`src/ui/modal/usage/mod.rs:27-366`, `render.rs:30-114`) ainda trava `std::sync::Mutex` a cada `ticker.tick()` (`is_loading()`) e `render()`; trocar por estado owned na UI alimentado por `watch` / `mpsc::try_recv` (seguindo o padrão de `FileSearchIndex` / `HistorySnapshot`).
  - `OpenCodeEngine::interrupt` (`src/infra/engine/opencode.rs:153-162`) ainda trava `std::sync::Mutex` (`pgid_guard`) sincronamente na thread da UI via `apply_sync_action`; mover para canal/atomics ou despacho não bloqueante.
- [ ] **Eliminar I/O / bloqueio síncrono restante em startup e `Up` do histórico:**
  - `App::new` (`src/ui/app/mod.rs:140`) e `src/cli.rs:190` ainda chamam `config_store::load()` sincronamente (incluindo `fs::write` se `needs_repair`).
  - `PromptHistory::ensure_entries_loaded` (`src/infra/history/mod.rs:271-292`) mantém fallback síncrono `done_rx.recv_timeout(2000ms)` em `KeyCode::Up` (`src/ui/app/events/keyboard.rs:374`) caso o worker inicial não tenha terminado.

### 4.4 Higiene de Commits e Paridade Documental — Corrigir Antes do `In Review`
- [ ] **Fatiar os commits da Fase 2 respeitando o limite atômico:**
  - O working tree possui 56 arquivos rastreados modificados + 8 novos arquivos/diretórios, falhando no pré-check padrão de `quality_gate.sh --full` sem `ATOMIC_OVERRIDE=1`. Organizar em commits lógicos (<= 5 arquivos por commit ou `ATOMIC_OVERRIDE=1` autorizado).
- [ ] **Sincronizar documentação com os módulos reais da Fase 2:**
  - [`.agents/docs/structure.md`](./structure.md): adicionar `src/core/usage/{mod,opencode_parse,parse}.rs`, `src/infra/{config_store,fs_search,model_catalog}.rs`, `src/infra/usage/{mod,cli,fetch,opencode_api,stats}.rs`, `src/infra/engine/process_tests.rs`, `src/infra/history/{api,hydrate,migrations}.rs`, `src/ui/app/{respawn_io,respawn_tests}.rs`, `src/ui/modal/model_effort/providers.rs` e os arquivos de teste novos (`scale_baseline.rs`, `migrations.rs`, `shortcuts.rs`, `acp_lifecycle.rs`, `mode_commands.rs`, `respawn_regression.rs`, `resume_telemetry.rs`, `sidebar_toggle.rs`, `input_tail.rs`, `rendering_tail.rs`); remover `src/ui/modal/usage/{cli,quota_parse,types}.rs` deletados.
  - [`.wiki/Frontal-Architecture.md`](../../.wiki/Frontal-Architecture.md): atualizar responsabilidades movidas de `core/config.rs`, `core/model_catalog.rs` e `ui/modal/usage/` para `infra/config_store.rs`, `infra/model_catalog.rs`, `infra/fs_search.rs`, `infra/usage/` e `core/usage/`, além de listar `hydrate.rs` e `migrations.rs` em `history/`.
  - [`.agents/docs/architecture_roadmap.md`](./architecture_roadmap.md): refletir status concluído das Fases 0–2 após fechar os itens acima.

---

## 5. Pontos de Atenção Pré-Mapeados — Fases 3 e 4

- **Fase 3 (`BackendAdapter`):** o backfill da coluna `backend` no SQLite (já iniciado na migração v1) deve ser verificado em todas as rotas de `/resume` e `external.rs` antes de apagar heurísticas `"ses_"` (18 ocorrências em 7 arquivos, incluindo a última entrada da `LEGACY_ALLOWLIST` em `src/ui/app/respawn.rs`) e `"/"`; aproveitar para remover campos mortos de `ModelEffortModal` (`models`, `ModelEffortStep::SelectEffort`) e padronizar `CommandRegistry`.
- **Fase 4 (Otimização 60 FPS):** validar o ganho diretamente no `scale_baseline.rs` evoluído (parse coalescido 1x/frame + prefix-sum $O(\log B)$ + `has_unconsolidated_blocks()` $O(1)$ + cache de layout no `PromptEditor` + eviction do `l2_cache`/highlighters/undo `paste_registry` + cota/hash forte no cache de mídia/PNG + extração clipboard via L1 com correção de coordenadas do rodapé/spinner).
