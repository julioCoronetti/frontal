# Frontal Architecture Audit Findings & Follow-Up Tracker (Parte 2)

**Documento complementar de:** [`architecture_roadmap.md`](./architecture_roadmap.md)  
**Última atualização:** Fase 1 revalidada na `develop` (`quality_gate.sh --full`: 2 unitários + 323 testes de integração aprovados).  
**Finalidade:** Registrar apontamentos abertos, débitos parciais e riscos verificados por fase para impedir que itens críticos sejam esquecidos ao avançar para as próximas etapas.

---

## 1. Painel de Notas por Fase

| Escopo | Nota | Estado | Resumo Objetivo |
| :--- | :---: | :--- | :--- |
| **Fase 0** (Blindagem & Scale Baseline) | **8.5 / 10** | Commitada (`4b63d30`, `3dc4cca`) | Ratchet bidirecional sólido em `boundaries.rs`; `scale_baseline.rs` ainda mede apenas misses L2, não trabalho $O(M^2)$/$O(B)$. |
| **Fase 1** (Ciclo de Vida & Estado Único) | **9.0 / 10** | Aprovada no `--full` (323/323 OK) | `Deref` removido, `TurnState` único, `agent_cache` (`RwLock`) removido da UI via `HistorySnapshot`, `killpg` 100% em `ProcessGroupGuard`, `SessionConfig(pub EngineConfig)`, migrações `PRAGMA user_version = 1` e docs sincronizados. |
| **Média das Entregas (Fases 0–1)** | **8.8 / 10** | — | Arquitetura geral evoluiu de **5.5/10** -> **7.5/10** (pronta para Fase 2). |

---

## 2. Apontamentos Abertos — Fase 0 (Cobrar na Fase 2 / Fase 4)

- [ ] **Métricas reais de trabalho em `tests/domains/cache_and_markdown/scale_baseline.rs`** *(cobrar na Fase 4)*:
  - Atualmente os 4 testes verificam apenas `cache_misses() <= 1` e contagem total de linhas, passando mesmo com re-parse $O(M^2)$ no streaming e scans $O(B)$ em `visible_slice` / `has_unconsolidated_blocks`.
  - Adicionar contadores/asserções para: número de invocações ao parser Markdown por burst de deltas (parse coalescido 60 Hz), tempo/operações em `visible_slice` para sessão grande ($O(\log B)$) e teto de entradas retidas no `l2_cache` após sequência de resizes.
- [ ] **Cobertura adicional em `tests/domains/architecture/boundaries.rs`** *(cobrar na Fase 2/3)*:
  - `has_static_mutex_or_rwlock` (`boundaries.rs:32-57`) não detecta `Mutex`/`RwLock` ocultos atrás de `type` alias.
  - `scan_core_violations` (`boundaries.rs:59-91`) não detecta literais `"opencode.db"` presentes em `src/core/model_catalog.rs:28,35`.
  - `scan_ui_violations` (`boundaries.rs:104-140`) ainda não bloqueia `"history.db"`, `"conversation_summaries.db"`, `"antigravity-oauth-token"` (`src/ui/modal/usage/{cli.rs:122,fetch.rs:145}`) nem reexport `pub use crate::infra::history::*;` (`src/ui/history.rs:4`).

---

## 3. Histórico & Residuais — Fase 1

### 3.1 Máquina de Estados `TurnState` (`src/ui/tab.rs`)
- [x] Eliminar `waiting_for_response` paralelo e derivar `is_waiting_for_response()` de `TurnState::Submitting`.
- [x] Fechar desvios de `transition_turn` em `stream.rs`, `clear_active_tab`, `close_or_reset_tab` e `cli.rs`, além de tratar `TurnState::Interrupting` em `StreamActivity` e limpar `in_flight_usage` em `TurnCompleted`.
- [ ] *(Menor / Fase 2)* `TurnState::Streaming { started_at }` ainda não é lido externamente (a UI lê `tab.turn_started_at`).

### 3.2 Centralização SQLite & Split-Brain de Histórico (`src/infra/history/`)
- [x] Remover `agent_cache: Arc<RwLock<Vec<SessionRecord>>>` da UI (`PromptHistory`), mover para o worker e conectar `HistorySnapshot` (`sessions` + `agent_sessions`) em `resume.rs`, `respawn.rs` e `sessions.rs`.
- [x] Aplicar `PRAGMA synchronous = NORMAL;` no `bg_conn` (`worker.rs`), unificar precedência de títulos (`>=` via `TitleSource::can_override`) e checar `cid` normalizado em `sync.rs`.
- [ ] *(Cobrar na Fase 2)* Eliminar cópias locais `self.sessions` e `self.deleted_ids` do lado da UI em `PromptHistory` (`publish_local_snapshot`) para que apenas o worker publique o estado canônico (usando `subscribe()` / canal assíncrono), e consolidar conexões read-only avulsas (`hydrate.rs`, `messages.rs`, `telemetry.rs`, `agent.rs`, `opencode/mod.rs`) mais abertura síncrona em `open_with_external_dbs`.

### 3.3 `ProcessGroupGuard`, `SessionConfig`, `reconfigure` e Docs
- [x] Eliminar todas as chamadas diretas a `libc::killpg` fora de `src/infra/engine/process.rs` (`usage/cli.rs`, `opencode.rs`, `acp/mod.rs`, `coordinator.rs`) e sinalizar `SIGTERM` na saída natural de `opencode.rs` e `clipboard/platform.rs`.
- [x] Corrigir `AcpEngine::reconfigure` (`shutting_down = true` apenas após `Self::spawn` bem-sucedido).
- [x] Eliminar duplicação dos 11 campos entre `SessionConfig` e `EngineConfig` (`SessionConfig(pub EngineConfig)`), usar `default_history_path()` em `App::new` e sincronizar `AGENTS.md`, `README.md`, `structure.md` e `.wiki/`.
- [ ] *(Residual / Fase 2)* Endurecer `ProcessGroupGuard::drop` (`src/infra/engine/process.rs`) caso seja acionado durante teardown de runtime Tokio (onde a task de 250ms do `REAPER_TX` pode ser cancelada) ou em thread OS sem runtime Tokio (0ms de graça antes do `SIGKILL`), e adicionar `#[cfg(not(unix))]` kill explícito no timeout de `usage/cli.rs`.
- [ ] *(Menor)* Citar explicitamente `hydrate.rs` e `migrations.rs` na descrição de `src/infra/history/` em `.agents/docs/structure.md` e `.wiki/Frontal-Architecture.md`.

---

## 4. Pontos de Atenção Pré-Mapeados — Fases 2, 3 e 4

- **Fase 2 (`tokio::select!` não bloqueante):** toda operação movida para background (`spawn`, `shutdown`, `respawn`, `send_prompt` + Base64, `hydrate`, `quota`) **deve** carregar `(tab_id, generation_id)` monotônico para descartar conclusões obsoletas fora de ordem; caches de `/quota` devem perder `static` global e virar estado de instância injetado em `infra/usage/`.
- **Fase 3 (`BackendAdapter`):** o backfill da coluna `backend` no SQLite (já iniciado na migração v1) deve ser verificado em todas as rotas de `/resume` e `external.rs` antes de apagar heurísticas `"ses_"` e `"/"`; aproveitar para remover campos mortos de `ModelEffortModal` (`models`, `ModelEffortStep::SelectEffort`) e padronizar `CommandRegistry`.
- **Fase 4 (Otimização 60 FPS):** validar o ganho diretamente no `scale_baseline.rs` evoluído (parse coalescido 1x/frame + prefix-sum $O(\log B)$ + eviction do `l2_cache`/highlighters/undo `paste_registry` + extração clipboard via L1 com correção de coordenadas do rodapé/spinner).
