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
4. **Governança Mecânica**: Limite estrito de 500 linhas por arquivo `.rs`, zero comentários no código-fonte, zero warnings no Clippy (`-D warnings`) e 150 testes de integração preservados.

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
| **Agy** | `default_fallback_models` | `AgyModelCatalog` | Catálogo de modelos do Agy/Gemini em `src/infra/transport.rs`. |
| **OpenCode** | *(Novo)* | `OpenCodeModelCatalog` | Catálogo dinâmico e fallback de modelos do OpenCode (DeepSeek, Claude, GPT) em `src/infra/transport.rs`. |

---

## 3. Especificação do Tradutor OpenCode (`OpenCodeStreamParser`)

O OpenCode v2.2.5 emite uma linha NDJSON por evento no `stdout` durante o comando `opencode run --format json --thinking`.

### Mapeamento de Eventos para `StreamEvent`

1. **`type: "step_start"`**:
   - Extrai `sessionID` (ex: `"ses_e33f0c..."`).
   - Mapeia para `StreamEvent::Init(InitEvent { conversation_id: Some(sessionID), init: Some(InitData::default()) })`.
   - Permite ao `SessionManager` e `TabState` registrar o ID da conversa no primeiro turno.

2. **`type: "text"`**:
   - Extrai `part.text`.
   - Mapeia para `StreamEvent::StepUpdate` com `step_type: Some(StepType::AgentResponse)` e `text_delta: Some(text)`.

3. **`type: "reasoning"` (ou `"thought"`)**:
   - Extrai `part.text`.
   - Mapeia para `StreamEvent::StepUpdate` com `step_type: Some(StepType::Thought)` e `thought: Some(text)`.
   - Renderiza no bloco colapsável de raciocínio.

4. **`type: "tool_use"`**:
   - Extrai `tool_name` e `call_id` (`part.callID` ou `part.id`).
   - Mapeia `call_id` para um `step_index: Option<u64>` determinístico via hash FNV-1a de 64 bits (prevenindo colisões e duplicações no `RenderCache`).
   - Extrai parâmetros e saída (`part.state.input` e `part.state.output`).
   - Converte status (`"running"` -> `StepState::Active`, `"completed"` -> `StepState::Done`, `"error"` -> `StepState::Error`).
   - Mapeia para `StreamEvent::StepUpdate` com `step_type: Some(StepType::Tool)` e `tool_info`.

5. **`type: "step_finish"` (Tratamento Crítico de Loop Agêntico)**:
   - Extrai contadores de tokens (`tokens.input`, `tokens.output`, `tokens.reasoning`, `tokens.cache.read`).
   - Extrai `part.reason`:
     - **Se `reason == "tool-calls"` (Passo Intermediário)**: Emite `StreamEvent::StepUpdate` com telemetria parcial de `usage`, **sem encerrar o turno**. O Frontal continua em modo `waiting_for_response`.
     - **Se `reason == "stop" | "length" | "end_turn"` (Passo Terminal)**: Emite `StreamEvent::Result(ResultEvent)` com status `Success` e o `TokenUsage` total acumulado.

6. **`type: "error"`**:
   - Mapeia para `StreamEvent::Error(ErrorEvent { error: Some(msg), message: None })`.

---

## 4. Ciclo de Vida do `OpenCodeEngine`

1. **Instanciação (`OpenCodeEngine::new`)**:
   - Não dispara processo OS na criação da aba (Lazy Spawning).
   - Emite imediatamente `StreamEvent::Init` com `conversation_id` inicial (se existente) para colocar a aba em `SessionStatus::Ready`.
2. **Execução de Turno (`send_prompt`)**:
   - Constrói argumentos CLI: `["run", "--format", "json", "--thinking", "--model", <model>]`.
   - Se `conversation_id` iniciar com `ses_...`, anexa `["--session", id]`.
   - Omitir `--session` no Turno 1; capturar `sessionID` emitido no primeiro evento e armazená-lo.
   - Instancia processo com `process_group(0)` e `PR_SET_PDEATHSIG(libc::SIGTERM)`.
   - Registra PGID em `ACTIVE_CHILD_PGIDS`.
3. **Término Normal de Turno**:
   - Quando o processo encerra com código `0`, emite `StreamEvent::Result` sintetizado (se o stream ainda não emitiu um terminal).
   - **Não emite `EngineEvent::ProcessExited`** em encerramentos normais, mantendo a aba ativa para o próximo turno.
   - `EngineEvent::ProcessExited` é reservado para crashes reais (código não-zero sem recuperação) ou fechamento explícito da aba (`shutdown()`).
4. **Interrupção e Encerramento**:
   - `interrupt()`: Envia `libc::killpg(pgid, libc::SIGINT)`.
   - `shutdown()`: Envia `killpg(pgid, libc::SIGTERM)` seguido de `SIGKILL`, desregistrando o PGID.

---

## 5. UI, Modais e Modelos (DeepSeek Support)

### A. Catálogo e Suporte a DeepSeek (`OpenCodeModelCatalog`)
O catálogo padrão do OpenCode inclui modelos de alta capacidade sem consumo de cotas pessoais do Google:
1. `opencode/deepseek-v4.1-flash-free` (DeepSeek V4.1 Flash — Gratuito / Cota OpenCode)
2. `deepseek/deepseek-chat` (DeepSeek Chat V3)
3. `deepseek/deepseek-reasoner` (DeepSeek R1 Reasoning)
4. `anthropic/claude-sonnet-4-5` (Claude Sonnet 4.5 via OpenCode)
5. `google/gemini-2.5-pro` (Gemini 2.5 Pro via OpenCode)
6. `openai/gpt-5` (GPT-5 via OpenCode)

### B. Modal `/model` (`ModelEffortModal`)
- **Seletor de Harness**: Barra visual no topo do modal:
  `[ 1: Antigravity (Google) ]    [ 2: OpenCode (DeepSeek/Multi-Provider) ]`
  *(Alternável via `Ctrl+H`, `F2` ou seleção numérica).*
- **Catálogo Dinâmico**: Ao alternar o harness, a lista de modelos é atualizada instantaneamente para o catálogo do motor correspondente.
- **Tratamento de Esforço**:
  - Para `Agy`: Exibe os níveis de raciocínio padrão (`Low`, `Medium`, `High`, `Max`).
  - Para `OpenCode`: Converte a etapa de esforço para controle da flag de raciocínio (`Thinking (Ativado)` vs `Padrão`).
- **Retrocompatibilidade de Ações**: `ModalAction::SelectModelEffort { model, effort }` permanece inalterado para preservar os testes unitários existentes; o `backend` selecionado é consultado pelo `App` diretamente no estado do modal.

### C. Modal `/quota` (`UsageModal`)
- **Inferência Contextual pela Aba Ativa**:
  - Se a aba ativa for `BackendKind::Agy`: Carrega a consulta OAuth do Google (`UsageModal::load`), exibindo cotas do Gemini.
  - Se a aba ativa for `BackendKind::OpenCode`: Carrega `UsageModal::for_opencode`, exibindo telemetria local:
    - *Context Window Capacity*: Percentual e tokens consumidos versus janela máxima do modelo.
    - *Prompt Cache Efficiency*: Taxa de acerto de cache (`cache_read_tokens`).
    - *Session Accumulated Tokens*: Total acumulado de entrada, saída e raciocínio na sessão.

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
  - `src/infra/transport.rs` (`opencode_bin`, `build_opencode_args`, `OpenCodeModelCatalog`)
  - `src/cli.rs` (argumento opcional `--opencode-bin`)
  - `tests/domains/protocol_and_cli.rs` (testes de parsing de CLI e catálogo)
- **Critério de Aceite**: Validação de binários via variáveis de ambiente e fallback para DeepSeek.

### Fatia 3: Motores de Execução (`AgyEngine`, `OpenCodeEngine` e `FrontalEngine`)
- **Arquivos**:
  - `src/infra/engine/mod.rs` (`AgyEngine` e aliases)
  - `src/infra/engine/opencode.rs` (`OpenCodeEngine`)
  - `src/infra/session.rs` (`FrontalEngine`, despacho em `create_subprocess_session`)
  - `tests/domains/engine_and_session.rs` (testes de mock de subprocesso por turno e killpg)
- **Critério de Aceite**: `OpenCodeEngine` executa turnos subsequentes reaproveitando `ses_...` sem fechar a sessão na saída do processo.

### Fatia 4: Integração de UI & Modais
- **Arquivos**:
  - `src/ui/tab.rs` (`backend: BackendKind` no `TabState`)
  - `src/ui/modal/model_effort/mod.rs` e `render.rs` (seletor de harness e catálogo dual)
  - `src/ui/modal/usage/mod.rs` (inferência contextual de `/quota`)
  - `src/ui/app/commands/modals.rs` (despacho contextual no App)
  - `tests/domains/tabs_and_modals/` (testes de alternância de backend)
- **Critério de Aceite**: Execução do quality gate completo (150+ testes, zero warnings, zero comentários, limite de 500 linhas).
