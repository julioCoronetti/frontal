# Frontal Multi-Harness Architecture & Implementation Plan
**Documento de Especificação Técnica e Roteiro de Execução**
**Versão:** 1.0.0 — **Status:** Aprovado em Auditoria do Argon Advisor
**Card de Referência:** Issue #27 (`feat(core): Suporte multi-backend com isolamento estrito (Agy & OpenCode)`)

---

## 1. Visão Geral e Princípios Arquiteturais

O Frontal foi concebido originalmente como uma TUI desacoplada para o Antigravity CLI (`agy`). Esta especificação expande o Frontal para um **harness universal e agnóstico de apresentação e controle**, permitindo conectar múltiplos motores agênticos (`FrontalEngine`) sem comprometer o motor padrão `agy` e sem alterar o contrato de renderização L1/L2 a 60 FPS.

### Princípios Inegociáveis
1. **Isolamento Estrito do Agy**: O motor padrão do Google (`AgyEngine`) permanece 100% intacto em seu caminho de execução, com default `BackendKind::Agy` em todas as configurações e testes. Zero risco de regressão ou detecção.
2. **Desacoplamento Visual pela Fronteira de Mensagens**: O núcleo da TUI (`TabState`, `RenderCache`, `MessageBlock`, `pulldown-cmark`, `Viewport`) consome exclusivamente a tipagem canônica `StreamEvent` via canal assíncrono `mpsc::Sender<SessionEnvelope>`. Nenhum componente de tela acessa binários ou protocolos externos diretamente.
3. **Semântica Adaptada por Motor**: O `AgyEngine` é um daemon persistente interativo via pipe `stdin`/`stdout`. O `OpenCodeEngine` é um supervisor de processos efêmeros **por turno** (`opencode run --format json --thinking`), cuja terminação normal de processo encerra o turno e mantém a sessão ativa.
4. **Governança Mecânica**: Limite estrito de 500 linhas por arquivo `.rs`, zero comentários no código-fonte, zero warnings no Clippy (`-D warnings`) e 175 testes de integração preservados.

---

## 2. Taxonomia e Topologia de Componentes

A nomenclatura interna é padronizada para distinguir claramente entre abstrações universais do Frontal e implementações específicas de cada harness:

### Mapeamento de Símbolos e Retrocompatibilidade

| Escopo | Símbolo Atual | Novo Símbolo Padronizado | Função e Arquivo Destino |
| :--- | :--- | :--- | :--- |
| **Genérico** | `SessionEngine` | `FrontalEngine` | `enum FrontalEngine { Agy, OpenCode, Replay }` em `src/infra/session.rs`. `pub type SessionEngine = FrontalEngine;` mantido como alias. |
| **Agy** | `SubprocessEngine` | `AgyEngine` | Motor persistente em `src/infra/engine/mod.rs`. `pub type SubprocessEngine = AgyEngine;` mantido como alias. |
| **OpenCode** | *(Novo)* | `OpenCodeEngine` | Motor por turno em `src/infra/engine/opencode.rs`. |
| **Genérico** | *(Novo)* | `BackendKind` | `pub enum BackendKind { Agy, OpenCode }` (default `Agy`) em `src/core/config.rs`. |
| **Agy** | `parse_stream_line` | `AgyStreamParser` | Decodificador do Agy em `src/core/protocol/agy.rs` (re-exportado na raiz de `protocol`). |
| **OpenCode** | *(Novo)* | `OpenCodeStreamParser` | Tradutor de NDJSON do OpenCode em `src/core/protocol/opencode.rs`. |
| **Agy** | `EngineConfig::default_fallback_models` | *(inalterado)* | Fallback estático de modelos Gemini do Agy em `src/infra/transport/mod.rs`. |
| **OpenCode** | *(Novo)* | `parse_opencode_models_output` / `fetch_opencode_models` | Descoberta via `opencode models` e parser ANSI em `src/infra/transport/opencode.rs`, sem lista estática. |

---

## 3. Especificação do Tradutor OpenCode (`OpenCodeStreamParser`)

O OpenCode v2.2.5 emite uma linha NDJSON por evento no `stdout` durante o comando `opencode run --format json --thinking`.

### Mapeamento de Eventos para `StreamEvent`

O contrato canônico é o enum existente em `src/core/protocol/mod.rs`: `Init`, `StepUpdate`, `Result`, `Error`, `Unknown`. Não existem variantes `TextDelta` ou `ToolCallStart`; todo delta textual e de ferramenta trafega em `StepUpdate`.

1. **`type: "step_start"`**:
   - Extrai `sessionID` (ex: `"ses_e33f0c..."`).
   - Mapeia para `StreamEvent::Init(InitEvent { conversation_id: Some(sessionID), init: None })`.
   - Permite ao `SessionManager` e `TabState` registrar o ID da conversa no primeiro turno.

2. **`type: "text"`**:
   - Extrai `part.text`.
   - Mapeia para `StreamEvent::StepUpdate` com `step_type: Some(StepType::AgentResponse)` e `text_delta: Some(text)`.
   - Texto vazio ou ausente produz `Ok(None)`.

3. **`type: "reasoning"` (ou `"thought"`)**:
   - Extrai `part.text`.
   - Mapeia para `StreamEvent::StepUpdate` com `step_type: Some(StepType::Thought)` e `thought: Some(text)`.
   - Renderiza no bloco colapsável de raciocínio.

4. **`type: "tool_use"`**:
   - Extrai `tool_name` (`part.tool`) e `call_id` (`part.callID` ou `part.id`).
   - Se `call_id` for não vazio, converte para `step_index: Some(u64)` determinístico via FNV-1a de 64 bits. Se ausente ou vazio, emite `step_index: None`, permitindo o fallback de correlação por `(nome, StepState::Active)` no `RenderCache`.
   - Extrai parâmetros e saída (`part.state.input` e `part.state.output`), preservando `part.state.error` no estado de erro.
   - Converte status (`"pending"`/`"running"` -> `StepState::Active`, `"completed"` -> `StepState::Done`, `"error"` -> `StepState::Error`).
   - Mapeia para `StreamEvent::StepUpdate` com `step_type: Some(StepType::Tool)`, `tool_name`, `tool_info` e `state`.

5. **`type: "step_finish"` (Tratamento Crítico de Loop Agêntico)**:
   - Extrai contadores de tokens (`tokens.input`, `tokens.output`, `tokens.reasoning`, `tokens.cache.read`).
   - O parser é puro e sem estado: sempre emite `StreamEvent::StepUpdate` com `usage` do passo e `step_type: Some(StepType::AgentResponse)`.
   - A acumulação entre passos pertence ao motor (`OpenCodeEngine`), que soma cada `usage` de passo em `turn_usage` e emite o `StreamEvent::Result` terminal com o total do turno. O parser nunca sintetiza `Result`.
   - `part.reason` não altera o contrato do parser; a distinção `tool-calls` vs `stop|length|end_turn` é decidida pela terminação do processo no motor.

6. **`type: "error"`**:
   - Extrai a mensagem de `error.data.message`, depois `error.message`, `error.type` ou `error.name`.
   - Mapeia para `StreamEvent::Error(ErrorEvent { error: Some(msg), message: None })`.

7. **`sessionID` fora de `step_start`**:
   - O identificador de sessão é lido de qualquer evento: `sessionID` no topo ou `part.sessionID`, desde que não vazio. O motor atualiza `conversation_id` a cada evento que carregue o campo, não apenas no `step_start`.

---

## 4. Ciclo de Vida do `OpenCodeEngine`

1. **Instanciação (`OpenCodeEngine::new`)**:
   - Não dispara processo OS na criação da aba (Lazy Spawning).
   - Emite imediatamente `StreamEvent::Init` com `conversation_id` inicial (se existente) para colocar a aba em `SessionStatus::Ready`.
2. **Execução de Turno (`send_prompt`)**:
   - Constrói argumentos CLI a partir da assinatura real do binário verificado (`opencode run --help`, v2.2.7):
     `["run", "--format", "json", "--thinking", "--auto", "--model", "<provider>/<model>#<variant>", "--", "<prompt>"]`.
   - `--thinking` apenas solicita a emissão dos blocos de raciocínio; **não** controla esforço.
   - O esforço é o sufixo `#<variant>` de `--model` (formato documentado pelo CLI: `provider/model#variant`). Mapeamento: `Low -> low`, `Medium -> high`, `High -> max`. Se o modelo já contiver `#`, o valor é passado verbatim.
   - `--auto` evita bloqueio por aprovação de ferramentas em execução não interativa.
   - Se `conversation_id` iniciar com `ses_...`, anexa `["--session", id]`. Omitir `--session` no Turno 1; capturar `sessionID` de qualquer evento subsequente e armazená-lo.
   - Instancia processo com `process_group(0)`, `PR_SET_PDEATHSIG(libc::SIGTERM)` e `stdin` em `Stdio::null()`.
   - Registra PGID em `ACTIVE_CHILD_PGIDS` no spawn e desregistra imediatamente no reap do processo do turno.
   - O `Child` pertence à task supervisora do turno; o motor guarda apenas o PGID em `AtomicI32`, evitando mutex mantido através de `wait()`.
3. **Término Normal de Turno**:
   - Quando o processo encerra com código `0`, emite `StreamEvent::Result` sintetizado com o `TokenUsage` acumulado do turno (se o stream ainda não emitiu um terminal).
   - **Não emite `EngineEvent::ProcessExited`** em encerramentos normais, mantendo a aba ativa para o próximo turno.
   - `EngineEvent::ProcessExited` é reservado para crashes reais (código não-zero sem recuperação) ou fechamento explícito da aba (`shutdown()`).
   - Saída não-zero sem `StreamEvent::Error` prévio emite `StreamEvent::Error` com a cauda de `stderr`.
4. **Interrupção e Encerramento**:
   - `interrupt()`: Envia `libc::killpg(pgid, libc::SIGINT)` e garante um evento terminal (`StreamEvent::Result` com `is_error`) para desbloquear a aba.
   - `shutdown()`: Envia `killpg(pgid, libc::SIGTERM)` seguido de `SIGKILL`, desregistrando o PGID.
5. **Invariante de Não-Bloqueio da Aba**:
   - `StreamEvent::Error` transiciona `SessionStatus::RunningTurn` -> `SessionStatus::Ready` em `SessionManager::recv()` e consolida os blocos parciais ativos no `TabState`.
   - `interrupt()` também restaura `SessionStatus::Ready`.
   - Nenhum caminho (erro, interrupção, saída não-zero) pode deixar `is_turn_active()` verdadeiro indefinidamente.

---

## 5. UI, Modais e Modelos (DeepSeek Support)

### A. Catálogo de Modelos (descoberta em `opencode models`)
- O catálogo é **derivado do binário em tempo de execução** por `opencode models`, que emite uma linha `provider/model` por modelo disponível. Nenhuma lista de modelos é mantida à mão na especificação ou no código: listas estáticas defasam e geram `--model` inválido.
- O parsing é puro (`parse_opencode_models_output`) e testável: ignora linhas vazias, códigos ANSI e entradas sem exatamente um separador `provider/model`.
- A descoberta roda uma única vez por sessão de TUI, em task assíncrona dedicada, e é armazenada no `App`; nunca executa na thread de render/input, preservando a invariante de event loop não bloqueante.
- Falha de descoberta (binário ausente ou saída vazia) produz catálogo vazio e mensagem explícita na UI; não há substituição silenciosa por modelos inventados.

### B. Modal `/model` (`ModelEffortModal`)
- **Seleção unificada por provedor**: o modal abre na lista de provedores, com `antigravity` (Agy) e cada provedor do OpenCode derivado do catálogo, exibindo a contagem de modelos.
- **Navegação**: `Enter` no provedor define o backend e o escopo; `Esc` no nível de modelos retorna à lista de provedores; `Esc` nos provedores fecha o modal.
- **Busca global**: qualquer texto digitado pesquisa simultaneamente nos catálogos Agy e OpenCode, e cada resultado exibe o provedor alinhado à direita. No nível de modelos do provedor, a coluna de provedor é omitida.
- **Layout**: modal ampliado (56% x 62%, limites 48×16 a 72×26) para acomodar a lista e a coluna de provedor; o slider de esforço continua oculto no nível de provedores.
- **Nomes de exibição**: provedores e modelos são formatados para leitura (`Antigravity`, `OpenCode Go`, `DeepSeek V4.1 Flash`), enquanto ids permanecem em slug minúsculo (`agy`, `opencode-go`, `opencode-go/deepseek-v4.1-flash`).
- **Tratamento de Esforço**:
  - Para `Agy`: níveis de raciocínio existentes (`Low`, `Medium`, `High`, `Max`) mapeados para `--effort`.
  - Para `OpenCode`: o esforço vira o sufixo `#variant` de `--model` (`Low -> low`, `Medium -> high`, `High -> max`). `--thinking` é sempre habilitado para receber raciocínio.
- **Retrocompatibilidade de Ações**: `ModalAction::SelectModelEffort { model, effort }` permanece inalterado; o `backend` selecionado é consultado pelo `App` no estado do modal.

### C. Modal `/quota` (`UsageModal`)
- **Inferência Contextual pela Aba Ativa**:
  - Se a aba ativa for `BackendKind::Agy`: carrega a consulta OAuth do Google (`UsageModal::load`), exibindo cotas do Gemini.
  - Se a aba ativa for `BackendKind::OpenCode`: carrega `UsageModal::for_opencode`, com telemetria local:
    - *Context Window Capacity*: `last_turn_usage.input_tokens + last_turn_usage.cache_read_tokens` sobre `context_window_for_model(model)`. Mede o turno mais recente, nunca o acumulado da sessão.
    - *Prompt Cache Efficiency*: `accumulated_usage.cache_hit_percentage()`.
    - *Session Accumulated Tokens*: `accumulated_usage` do `TabState`.

---

## 6. Roteiro de Implementação em Fases Atômicas

Cada fatia altera no máximo 5 arquivos e é validada via `quality_gate.sh`:

### Fatia 1: Particionamento de Protocolo & Tradutor OpenCode
- **Arquivos**:
  - `src/core/protocol/mod.rs` (re-exports canônicos)
  - `src/core/protocol/agy.rs` (isolamento do parser Agy)
  - `src/core/protocol/opencode.rs` (novo `OpenCodeStreamParser`)
  - `tests/domains/protocol_and_cli.rs` (testes de decodificação de fixtures NDJSON)
- **Critério de Aceite**: Compilação sem warnings, 100% dos testes existentes passando e novos testes unitários para eventos `text`, `reasoning`, `tool_use` e `step_finish`.

### Fatia 2: Configuração, Transporte & Catálogos Duplos
- **Arquivos**:
  - `src/core/config.rs` (`BackendKind`)
  - `src/infra/transport/{mod.rs, opencode.rs}` (`opencode_bin`, `build_opencode_args`, descoberta de modelos)
  - `src/cli.rs` (argumento opcional `--opencode-bin`)
  - `tests/domains/protocol_and_cli.rs` (testes de parsing de CLI e catálogo)
- **Critério de Aceite**: Validação de binários via variáveis de ambiente e fallback para DeepSeek.

### Fatia 3: Motores de Execução (`AgyEngine`, `OpenCodeEngine` e `FrontalEngine`)
- **Arquivos**:
  - `src/infra/engine/mod.rs` (`AgyEngine` e aliases)
  - `src/infra/engine/opencode.rs` (`OpenCodeEngine`)
  - `src/infra/session.rs` (`FrontalEngine`, despacho em `create_subprocess_session`)
  - `tests/domains/engine_and_session/` (testes de mock de subprocesso por turno e killpg)
- **Critério de Aceite**: `OpenCodeEngine` executa turnos subsequentes reaproveitando `ses_...` sem fechar a sessão na saída do processo.

### Fatia 4: Integração de UI & Modais
- **Arquivos**:
  - `src/ui/tab.rs` (`backend: BackendKind` no `TabState`)
  - `src/ui/modal/model_effort/mod.rs` e `render.rs` (seletor de harness e catálogo dual)
  - `src/ui/modal/usage/mod.rs` (inferência contextual de `/quota`)
  - `src/ui/app/commands/modals.rs` (despacho contextual no App)
  - `tests/domains/tabs_and_modals/` (testes de alternância de backend)
- **Critério de Aceite**: Execução do quality gate completo (175 testes, zero warnings, zero comentários, limite de 500 linhas).

---

## 7. Histórico e Retomada (`/resume`)

- **Persistência**: ao fim de cada turno, os blocos consolidados (`user`, `assistant`, `thought`, `tool`) são gravados na tabela `messages` do `history.db`, com watermark por aba para evitar duplicação.
- **Hidratação**: `hydrate_conversation` tenta primeiro o transcript do Agy (`~/.gemini/antigravity-cli/brain/<id>/...`) e, na ausência, reconstrói o `RenderCache` a partir do store do Frontal. Isso cobre sessões `ses_...` do OpenCode sem depender do formato privado de terceiros.
- **Backend na retomada**: o `conversation_id` define o motor (`ses_` → OpenCode, UUID → Agy). A sessão estrangeira troca o backend em vez de ser descartada; trocas de modelo sem conversa continuam limpando ids incompatíveis.
- **Título**: consulta o resumo do Agy e, como fallback, a tabela `sessions` do Frontal.
- **Limite**: conversas iniciadas antes da persistência existir não têm transcript local e só reabrem quando houver transcript do Agy.
