# Frontal Architecture Roadmap (Target 10/10) & 10-Bump Audit Protocol

**Baseline:** `develop` (`11f512e`, v0.21.0, nota **5.5 / 10**) vs `main` (`2b7dc80`, v0.21.0, nota **4.8 / 10**)  
**Escopo:** Plano de consolidação estrutural, escalabilidade e otimização 60 FPS, mais protocolo recorrente de auditoria a cada 10 bumps/atualizações.  
**Parte 2 (Apontamentos e Pendências por Fase):** [`architecture_audit_findings.md`](./architecture_audit_findings.md)

---

## 1. Premissa Original da Arquitetura e Causa Raiz dos Gaps

Pela especificação em `README.md` e `.wiki/Frontal-Architecture.md`, o Frontal foi concebido sob cinco pilares:

1. **Desacoplamento por fronteira de mensagens (`StreamEvent` / `SessionEnvelope`)**: isolar a apresentação TUI (`ratatui`/`crossterm`) de protocolos específicos de CLIs externos.
2. **Renderização determinística a 60 FPS**: cache em dois níveis (L1 AST imutável + L2 linhas quebradas por largura) para streaming $O(1)$ e loop `tokio::select!` livre de I/O bloqueante.
3. **Estado por sessão em `TabState`**: troca de aba em $O(1)$ sem `std::mem::swap`.
4. **Isolamento POSIX por grupo de processos**: `setpgid(0, 0)`, `PR_SET_PDEATHSIG` e escalonamento `killpg` contra processos órfãos.
5. **Governança mecânica**: teto de 500 linhas por `.rs`, zero comentários no código-fonte, `clippy -D warnings` e suíte hermética em 8 domínios.

### Veredito sobre a Premissa
A premissa original é **tecnicamente sólida e não requer reescrita**. A erosão observada após sucessivas correções decorreu de três lacunas de governança:

- **Teto de 500 linhas sem fiscalização de dependências:** `tests/domains/architecture.rs` verifica limite de linhas, comentários e paridade documental, mas não proíbe violações de direção de dependência (`core` fazendo I/O ou `ui` abrindo SQLite/subprocessos). Com isso, o crescimento forçou fatiamentos horizontais mantendo o acoplamento cruzado.
- **`App: Deref<TabState>` como invariante oficial:** a conveniência sintática de acessar a aba ativa via `Deref` diluiu a fronteira entre estado global do `App` e estado por aba, induzindo triplicação de estado (`App`, `TabState`, `SessionManager`) e regressões entre abas ativas e em background.
- **Falsa abstração multi-backend:** `FrontalEngine` abstrai apenas o transporte de stream, enquanto catálogo de modelos, modos, effort, prefixo `ses_`, hidratação e quota vazaram como ramificações binárias (`Agy` vs `OpenCode`) por UI, histórico e configuração.

---

## 2. Comparativo de Linha de Base (`main` vs `develop`)

| Dimensão | `main` (`2b7dc80`) | `develop` (`11f512e`) | Evolução Observada |
| :--- | :---: | :---: | :--- |
| **Escalabilidade / Desacoplamento** | 5.0 / 10 | 5.5 / 10 | Quebrou ciclo `infra <-> ui` (`core::media`), removeu `AgyEngine` legado e particionou arquivos no limite de 499 linhas; manteve acoplamento binário a 2 backends e aumentou sincronização paralela de estado. |
| **Otimização / Performance** | 4.5 / 10 | 5.5 / 10 | Eliminou double JSON parsing, DOM `Value` intermediário, `Ctrl+C` bloqueante no loop, `INSERT`s sem transação, polling no OpenCode e clonagem no `CommandsModal`; manteve `.await` inline no `select!` e re-parse $O(M^2)$ no streaming. |
| **Nota Geral** | **4.8 / 10** | **5.5 / 10** | Base superior na `develop`, com dívidas estruturais centrais mapeadas nas fases abaixo. |

---

## 3. Roadmap Consolidado para Nota 10 / 10

### Fase 0 — Blindagem Mecânica de Fronteiras e Baseline de Escala
*Pré-requisito para evitar novas regressões estruturais durante as refatorações.*

1. **Matriz de fronteiras de camada em `tests/domains/architecture.rs`:**
   - Proibir em `src/core/**`: `rusqlite`, `tokio::process`, `std::process`, `libc`, `std::fs`, `crate::infra`, `crate::ui`.
   - Proibir em `src/ui/**`: `rusqlite`, `tokio::process`, `std::process`, `libc`, `std::fs`, caminhos hardcoded (`opencode.db`, `transcript.jsonl`), heurísticas `"ses_"` e globais `static` com `Mutex`/`RwLock`.
2. **Baseline de escala em `tests/domains/cache_and_markdown/`:**
   - Adicionar teste determinístico (ex.: milhares de blocos, streaming em centenas de deltas, sequência de resizes) para validar ganho de complexidade e limite de alocações.

**Critério de pronto:** `cargo test --test suite domains::architecture` bloqueando violações de camada em CI/local.

---

### Fase 1 — Integridade de Ciclo de Vida e Fonte Única de Verdade
*Elimina vazamentos de processo, split-brain e estado divergente.*

1. **Guardião único de PGID (`ProcessGroupGuard` em `src/infra/engine/process.rs`):**
   - Centralizar registro/desregistro em `ACTIVE_CHILD_PGIDS` e limpeza `killpg` para todos os motores e subprocessos auxiliares.
   - Garantir desregistro e limpeza de grupo também na saída natural (`child.wait()` em `src/infra/engine/acp/coordinator.rs`).
   - No `Drop`, nunca executar `sleep` bloqueante em thread Tokio: disparar sinalização imediata não bloqueante ou delegar o escalonamento (`SIGINT` -> `SIGTERM` -> `SIGKILL`) a um reaper assíncrono dedicado.
2. **Remover `App: Deref<TabState>` e consolidar estado:**
   - Substituir `Deref`/`DerefMut` em `src/ui/app/mod.rs` por acessores explícitos (`active_tab()`, `tab(id)`, `tab_mut(id)`), mantendo `active_tab_index` como única fonte de seleção.
   - Unificar configuração ativa em `SessionConfig` (fonte única na aba/sessão), tornando `SessionMetadata` uma projeção somente-leitura para persistência e `/resume` (eliminando divergência com `EngineConfig` em `src/infra/session.rs`).
   - Consolidar os 6 pontos de reset de turno/erro/interrupção (`src/ui/tab.rs`, `src/ui/app/stream.rs`, `modals.rs`, `keyboard.rs`) em uma máquina de estados única `TurnState` via `TabState::transition_turn(...)`, removendo caminhos duplicados de `Interrupt` e `SwitchTab`.
3. **Centralizar histórico e título no worker WAL (`HistoryService`):**
   - Remover conexões SQLite ad-hoc em `src/ui/history.rs` e eliminar caches/conjuntos duplicados na UI (`deleted_ids`, `sessions`, precedências divergentes `>` vs `>=`).
   - Operar a UI por comandos assíncronos e snapshots versionados imutáveis publicados pelo worker; restringir `flush()` estritamente ao shutdown gracioso com timeout limitado.
   - Substituir `ALTER TABLE` avulsos no startup por migrações idempotentes via `PRAGMA user_version` e adicionar índices em `sessions(conversation_id)` e `sessions(created_at)`.

**Critério de pronto:** zero vazamentos em `ACTIVE_CHILD_PGIDS`, zero `Deref<TabState>`, estado único de turno/sessão e histórico acessado exclusivamente via `HistoryService`.

---

### Fase 2 — Event Loop 100% Não Bloqueante e Separação Estrita de Camadas
*Zera travamentos no `tokio::select!` e remove I/O de `core/` e `ui/`.*

1. **Extrair I/O de `src/core/` e `src/ui/` para `src/infra/`:**
   - Tornar `UserConfig` (`src/core/config.rs`) e `ModelCatalog` (`src/core/model_catalog.rs`) estruturas puras de domínio (sem `fs::read_to_string`/`save` em `Default` e sem `rusqlite` no `core`).
   - Mover subprocessos (`agy`, `curl`), leitura de token OAuth, queries em `opencode.db`, gestão de PGID e cache de `src/ui/modal/usage/*` para um serviço instanciado em `src/infra/usage/`, eliminando globais estáticos (`USAGE_CACHE`, `OPENCODE_USAGE_CACHE`, `OPENCODE_STATS_CACHE`).
2. **Operações pesadas fora do `tokio::select!` com `generation_id`:**
   - Mover `spawn`, `shutdown`, `respawn`, `send_prompt` (incluindo leitura e codificação Base64 de imagens em `spawn_blocking`), `hydrate` e `quota fetch` para tarefas assíncronas em background (`src/ui/mod.rs`).
   - Anexar `(tab_id, generation_id)` monotônico a toda operação em background para descartar respostas obsoletas caso o usuário feche a aba ou dispare novo `respawn`/`/model` antes da conclusão.
   - Eliminar aquisições síncronas de `std::sync::RwLock`/`Mutex` e flush síncrono de OSC 52 no loop da UI, trocando por canais ou snapshots `Arc`.

**Critério de pronto:** nenhum `.await` de ciclo de vida de processo ou I/O síncrono dentro dos braços do `tokio::select!`; isolamento total em testes paralelos sem estado global mutável.

---

### Fase 3 — Contrato Único Multi-Backend (`BackendAdapter`)
*Permite adicionar um 3º backend em um único módulo sem tocar em `ui/` ou `history/`.*

1. **Descritores em `core` + adaptadores em `infra/backends/`:**
   - Manter em `src/core/backend.rs` apenas identificadores e descritores de dados (`BackendKind`, capacidades, modos, políticas de effort e chave composta `SessionKey { backend, id }`).
   - Isolar `agy` e `opencode` em `src/infra/backends/{agy,opencode}` sob um registry único responsável por: fábrica de engine, descoberta de catálogo de modelos, mapeamento de mode/effort, hidratação/deleção de histórico e provedor de quota.
2. **Migração e limpeza de heurísticas:**
   - Executar backfill idempotente no SQLite garantindo a coluna `backend` preenchida em registros legados antes de remover todas as heurísticas de prefixo `ses_` e `/` fora de `infra/backends/`.
   - Unificar registro de comandos e modais (`CommandRegistry`) e remover campos/estados mortos em `ModelEffortModal` (`models`, `ModelEffortStep::SelectEffort`).

**Critério de pronto:** suportar um novo backend exige 1 módulo em `infra/backends/<novo>/` + 1 entrada no registry, sem alterar `ui/`, `core/model_catalog` ou `history/sessions`.

---

### Fase 4 — Otimização 60 FPS Real e Memória Limitada
*Elimina complexidade $O(M^2)$ / $O(B)$ e crescimento ilimitado em sessões longas.*

1. **Streaming incremental e coalescido no `RenderCache`:**
   - Em `append_streaming_delta`, apenas anexar o delta ao buffer bruto e marcar `dirty = true`, executando parse Markdown no máximo 1x por quadro de renderização (60 Hz).
   - Congelar blocos completos anteriores ao último delimitador estrutural seguro (`\n\n` fora de code fences) em L1, re-parseando apenas a cauda ativa.
2. **Consultas $O(1)$ / $O(\log B)$ e redução de alocações:**
   - Substituir varredura em `has_unconsolidated_blocks()` por contador/índice $O(1)$.
   - Manter vetor de soma de prefixos de linhas L2 (`cumulative_lines`) para `visible_slice` e hit-test de mouse em $O(\log B) + O(\text{viewport})$.
   - Extrair texto de seleção/clipboard diretamente dos dados estruturados de L1, eliminando reverse-parse de prefixos visuais (`"▌ "`, `"  │ "`, `"+ Thought"`) em `src/ui/cache/slice.rs` e corrigindo o desvio de coordenadas quando há rodapé/spinner ativo.
   - Cachear o layout do `PromptEditor` por `(rope_revision, width)` para evitar conversões `Rope -> String` quando não há edição.
3. **Retenção limitada de memória e disco:**
   - Reter `l2_cache` apenas para a largura ativa e definir limite/eviction para caches auxiliares (`code_highlighters`, `suspended_events`, `PromptEditor::undo_stack` sem clonar `paste_registry` inteiro a cada snapshot).
   - Aplicar cota/LRU em `~/.frontal/cache/media` com hash forte na deduplicação (substituindo `CRC32` isolado de 32 bits) e compressão/otimização de buffer no encoder PNG (`src/infra/clipboard/image_codec.rs`).

**Critério de pronto:** complexidade sublinear no viewport em sessões extensas, streaming sem degradação $O(M^2)$ e memória estável em sessões e resizes contínuos.

---

## 4. Protocolo de Auditoria Periódica (A Cada 10 Bumps / Releases)

Para evitar que correções incrementais degradem a arquitetura entre entregas, adotar um marco obrigatório a cada **10 atualizações de versão**:

### 4.1 Gatilho no Ciclo de Release (`card_lifecycle.sh`)
- Contabilizar tags semânticas `vX.Y.Z` a partir da baseline (`count % 10 == 0`).
- Ao atingir o 10º bump, bloquear a abertura de novas features até concluir um card dedicado:
  `chore(audit): structural health audit vX.Y.Z`.

### 4.2 Checklist Determinístico de 5 Dimensões
Critério mínimo de aprovação do marco: **zero achados `CRITICAL` e nota consolidada >= 9.0 / 10**.

1. **Fronteiras de Camada e Dependências:**
   - Executar `domains::architecture` validando imports proibidos, zero I/O em `core/`, zero SQLite/processos/globais mutáveis em `ui/`.
   - Avaliar se o volume do repositório justifica migrar o crate único para Cargo workspace (`frontal-core`, `frontal-infra`, `frontal-ui`) para bloqueio em tempo de compilação.
2. **Latência do Event Loop:**
   - Auditar `src/ui/mod.rs` confirmando zero `.await` inline de spawn/shutdown/respawn/send_prompt no `tokio::select!` e cobertura de `generation_id` em toda nova tarefa assíncrona.
3. **Complexidade e Consumo de Memória:**
   - Executar a suíte de escala (`cache_and_markdown`) validando `visible_slice` $O(\log B)$, parse coalescido no streaming, zero alocações do editor em repouso e teto de memória/disco nos caches.
4. **Isolamento Multi-Backend:**
   - Verificar via busca automatizada ausência de ramificações por `BackendKind`, `ses_` ou strings de protocolo fora de `infra/backends/` e `core/protocol/`.
5. **Ciclo de Vida e Paridade Documental:**
   - Auditar cobertura de `ProcessGroupGuard` em todos os spawns de processo, confirmar convergência de `TurnState`/`SessionConfig` e sincronizar `AGENTS.md`, `README.md` e `.wiki/Frontal-Architecture.md` com a implementação real.

---

## 5. Parte 2 — Registro Vivo de Apontamentos e Pendências

Todos os achados abertos por fase (itens parciais, gaps residuais e verificações para etapas futuras) estão consolidados e rastreados em [`architecture_audit_findings.md`](./architecture_audit_findings.md).
