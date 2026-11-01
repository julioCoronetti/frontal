# Diretrizes do Agente - Repositório Frontal

## 1. Visão Geral do Projeto
- **Projeto**: Frontal.
- **Objetivo**: Interface de usuário em terminal (TUI) desacoplada, de alta performance e assíncrona para o Antigravity CLI (`agy`) e motores compatíveis com o protocolo `stream-json`.
- **Status do Projeto**: Arquitetura em Rust implementada e operacional. As Fases 1 a 4 do roadmap foram integralmente concluídas na branch `main` e consolidadas por suíte unificada de integração com 7 domínios (83 testes). O sistema encontra-se em fase de manutenção contínua e evolução de features.
- **Stack Oficial Ativa (Crates do `Cargo.toml`)**:
  - `tokio` (features: `rt-multi-thread`, `macros`, `process`, `io-util`, `sync`, `time`, `signal`, `fs`)
  - `tokio-util` (feature: `codec`)
  - `bytes`, `futures-util` (feature: `alloc`)
  - `serde`, `serde_json`, `libc`
  - `ratatui` (0.29), `crossterm` (0.28, feature: `event-stream`)
  - `pulldown-cmark` (0.12), `syntect` (5.2, feature: `default-fancy`)
  - `ropey` (1.6), `rusqlite` (0.32, feature: `bundled`), `arboard` (3.4)
  - *Restrição Estrita:* O projeto é um pacote único (*single-crate*). O crate `portable-pty` não integra a compilação atual. Nenhuma dependência externa deve ser adicionada ao `Cargo.toml` sem autorização prévia explícita.
- **Branches de Referência**:
  - `main`: Branch ativa para a arquitetura do harness em Rust.
  - `origin/legacy`: Código integral e testes do protótipo inicial (Python / Textual).
- **Papel do Agente**: Atuar como engenheiro de software no desenvolvimento de features, correções de bugs, refatoração e testes, garantindo qualidade, estabilidade e conformidade com a arquitetura definida.

---

## 2. Princípios de Engenharia e Invariantes Arquiteturais

### Princípios Fundamentais
1. **Zero Comentários no Código:** É terminantemente proibido inserir comentários no código-fonte. Toda documentação, contexto e justificativas devem residir na Wiki do GitHub, na documentação formal ou nas mensagens de commit.
2. **Simplicidade e Modularidade:** Código limpo, desacoplado, modular e sem over-engineering. Nomes descritivos padronizados em inglês. Sem prints residuais, logs de depuração ad-hoc ou chamadas soltas.
3. **Qualidade e Testes:** Todo código deve aderir rigorosamente às ferramentas de compilação, linting e testes da stack Rust.
4. **Limite de Complexidade e Modularidade de Arquivos:** Teto formal de **500 linhas** por arquivo `.rs`. Ultrapassando o teto, o módulo deve ser decomposto via blocos `impl` particionados em subdiretórios homônimos (como em `src/ui/app/`), mantendo re-exports públicos e zero quebra de API. Não criar camadas arbitrárias de indireção (*over-engineering* estilo Clean Architecture pesada em Rust).

### Invariantes Arquiteturais Obrigatórios
1. **Cache de Renderização em 2 Níveis ($O(1)$ no turno ativo):**
   - O Nível 1 preserva a AST semântica imutável (`MessageBlock` com spans de estilo em `src/ui/cache.rs`) para turnos consolidados.
   - O Nível 2 mantém linhas visuais pré-formatadas indexadas por `(width, revision)`. Redimensionamentos (`SIGWINCH`) ou expansão/colapso de blocos invalidam seletivamente apenas o Nível 2 sem reprocessar Markdown.
   - Durante streaming contínuo, apenas o bloco ativo emite deltas, garantindo taxa de quadros estável a 60 FPS independentemente do volume histórico da conversa.
2. **Event Loop TUI Não-Bloqueante (60 FPS):**
   - O loop assíncrono `tokio::select!` em `src/ui/mod.rs` jamais deve executar operações bloqueantes de I/O de disco ou chamadas de rede síncronas na thread de renderização.
   - Varreduras de arquivos no disco (`FileSearchWorker`), persistência em SQLite (`PromptHistory`) e descoberta de binários/modelos no `PATH` operam fora da thread de renderização. Comandos locais (`/help`, `/clear`, `/model`, `/effort`, `/diff`, `/quit`) são interceptados na UI antes do transporte IPC.
3. **Sincronização de Estado Multisessão (`App` ↔ `TabState` via `std::mem::swap`):**
   - O estado da sessão ativa reside diretamente nos campos de `App` e é permutado via `std::mem::swap` nos métodos `save_active_tab_state` e `load_active_tab_state`. Abas em segundo plano processam deltas via `TabState::handle_background_stream_event`.
   - Qualquer novo campo de estado por aba introduzido em `App` deve obrigatoriamente ser espelhado em `TabState`, incluído no ciclo de swap e manipulado no processador de background para evitar vazamento ou perda silenciosa de dados ao alternar abas (`Alt+1..9`).
4. **Isolamento POSIX de Subprocessos:**
   - Todo subprocesso do motor agêntico (`agy`) deve ser instanciado em grupo de processo próprio via `setpgid(0, 0)` e registrar `PR_SET_PDEATHSIG(libc::SIGTERM)` no hook `pre_exec` no Linux (`src/transport.rs`).
   - O encerramento deve sinalizar o grupo de processos (`killpg`) com escalonamento ordenado (`SIGINT` -> `SIGTERM` -> `SIGKILL`) para impedir processos órfãos.
5. **Isolamento de Efeitos Colaterais em Testes:**
   - Testes automatizados que inicializarem persistência devem utilizar instâncias em memória (`PromptHistory::open_in_memory()`) para não poluir ou gerar conflito concorrente com `~/.frontal/history.db`. Clipboard em testes deve utilizar buffer isolado em memória (`SystemClipboard::new_isolated()`).

---

## 3. Mapeamento Estrutural do Repositório

### Núcleo & IPC (`src/`)
- `src/main.rs`: Ponto de entrada do executável da aplicação.
- `src/lib.rs`: Declaração dos módulos da biblioteca pública (`cli`, `config`, `engine`, `protocol`, `session`, `transport`, `ui`).
- `src/config.rs`: Configuração persistente do usuário (`~/.frontal/config.toml`, modelo, modo e esforço padrão).
- `src/cli.rs`: Parser de argumentos de linha de comando (`CliArgs`), orquestração do modo headless e inicialização do TUI.
- `src/protocol.rs`: Tipagem estrita e desserialização NDJSON do protocolo `stream-json` (`EngineEvent`, `StreamEvent`, `ToolInfo`, etc.).
- `src/transport.rs`: Implementação de `EngineTransport` (`SubprocessEngine` com buffer `LinesCodec` de 16 MB e `FixtureReplayEngine`).
- `src/engine.rs`: Abstração de alto nível do motor agêntico e canais de streaming.
- `src/session.rs`: Gerenciamento e multiplexação de sessões via `SessionId` e `SessionManager`.

### Interface de Terminal (`src/ui/`)
- `src/ui/mod.rs`: Loop assíncrono de eventos TUI (60 FPS, crossterm `EventStream`, cancelamento, ticks de renderização).
- `src/ui/app/`: Módulo central de aplicação particionado em subarquivos coesos (< 400 linhas):
  - `src/ui/app/mod.rs`: Struct `App`, enum `AppAction`, construtores (`new`, `new_isolated`), getters de metadados ativos e sincronização interna de inputs.
  - `src/ui/app/tabs.rs`: Gestão multisessão e isolamento visual do ciclo de swap (`std::mem::swap`).
  - `src/ui/app/events.rs`: Roteamento não-bloqueante de eventos do terminal (teclado, mouse, paste e autocompletação).
  - `src/ui/app/commands.rs`: Despacho de comandos locais (`/quit`, `/clear`, `/interrupt`, etc.), ações modais e extração de git diff.
  - `src/ui/app/stream.rs`: Processamento de envelopes assíncronos do motor IPC (`handle_engine_envelope`).
  - `src/ui/app/render.rs`: Métodos de desenho, layout e widgets da TUI a 60 FPS.
- `src/ui/tab.rs`: Gerenciamento de abas (`TabState`), swap de memória e processamento de background.
- `src/ui/cache/`: Cache de renderização em 2 níveis particionado (`mod.rs`, `format.rs`, `slice.rs`).
- `src/ui/viewport.rs`: Viewport da conversa com máquina de estados para sticky scroll e seleção limpa de texto.
- `src/ui/markdown/`: Parser incremental `pulldown-cmark` particionado (`mod.rs`, `normalize.rs`, `parse.rs`, `render.rs`).
- `src/ui/highlight.rs`: Realce sintático incremental por linha com `syntect` e checkpoints de estado.
- `src/ui/editor.rs`: Editor de prompt multilinha sobre B-Tree (`ropey`), extmarks atômicos (`@`, `/`) e pílulas de paste (`[Pasted ~N lines]`).
- `src/ui/autocomplete.rs`: Popover flutuante de autocompletação para comandos e arquivos (`FileSearchWorker`).
- `src/ui/history.rs`: Persistência de histórico de comandos em SQLite WAL (`PromptHistory`).
- `src/ui/modal/`: Pilha de diálogos modais flutuantes particionada (`mod.rs`, `diff.rs`, `model_effort.rs`, `palette.rs`, `respawn.rs`).
- `src/ui/diff.rs`: Visualizador de diffs unificados integrado.
- `src/ui/clipboard.rs`: Suporte à área de transferência do sistema via `arboard` com buffer isolado em memória e fallback OSC 52.
- `src/ui/notify.rs`: Notificações sensoriais de terminal (campainha auditiva e flashes visuais).
- `src/ui/terminal.rs`: Inicialização de modo raw, bracketed paste e panic hook para restauração de TTY.
- `src/ui/theme.rs`: Definições visuais e paleta de cores inspirada no OpenCode.
- `src/ui/input_stub.rs`: *Stub legado de entrada provisório preservado estritamente para compatibilidade de testes.*

### Suíte Unificada de Testes de Integração (`tests/`)
- `tests/suite.rs`: Ponto de entrada canônico do binário único de testes (*single integration test crate*).
- `tests/common/mod.rs`: Utilitários compartilhados herméticos (`test_app`, `press_key`, guards RAII `TempDirGuard`/`TempFileGuard` com gerador atômico de sequência, `PGID_TEST_LOCK`).
- `tests/domains/protocol_and_cli.rs`: Testes de protocolo `stream-json`, NDJSON, `LinesCodec` (16 MB) e flags CLI (11 testes).
- `tests/domains/engine_and_session.rs`: Testes de subprocesso `agy`, replay de fixtures, POSIX `killpg`/`setpgid` e multiplexação MPSC (9 testes).
- `tests/domains/cache_and_markdown.rs`: Testes de RenderCache L1/L2 $O(1)$, streaming Markdown, syntax highlighting `syntect`, tool cards e 60 FPS (16 testes).
- `tests/domains/viewport_and_clipboard.rs`: Testes de sticky scroll, seleção limpa, word-wrap UTF-8, OSC 52, notificações sensoriais e compatibilidade `MinimalInputStub` (8 testes).
- `tests/domains/editor_and_history.rs`: Testes de editor `ropey`, extmarks atômicos, paste pills, autocomplete popover e persistência SQLite WAL (8 testes).
- `tests/domains/tabs_and_modals.rs`: Testes de multissessão (`App` ↔ `TabState`), modais, paleta de comandos, hot-respawn, diff viewer e tool lifecycle em background (19 testes).
- `tests/domains/tui_integration.rs`: Testes fim-a-fim com `TestBackend`, comandos locais (`/quit`, `/clear`, `/help`, etc.), mouse routing, paste/multiline, simetria OpenCode e dimensões adversariais (11 testes).

---

## 4. Fontes de Informação e Documentação
Para contexto aprofundado, regras de negócio e histórico técnico, consultar:
1. **Documentação Local:**
   - `README.md`: Especificação técnica oficial da arquitetura em Rust, diretrizes de cache em 2 níveis, sticky scroll e detalhamento das fases concluídas.
   - `Cargo.toml`: Definição declarativa estrita das dependências ativas.
2. **Rastreamento de Demandas e GitHub:**
   - Repositório Git: `https://github.com/julioCoronetti/frontal`
   - Project Board: `https://github.com/users/julioCoronetti/projects/8` (Frontal)
   - Issues numeradas no GitHub para critérios de aceite detalhados.
3. **Documentação Oficial (Wiki do GitHub):**
   - Hub Central: `https://github.com/julioCoronetti/frontal/wiki` e `https://github.com/julioCoronetti/frontal/wiki/Home`
   - **Dossiê Antigravity CLI (`agy`):**
     - Visão Geral: `https://github.com/julioCoronetti/frontal/wiki/Agy`
     - Arquitetura & Protocolo: `https://github.com/julioCoronetti/frontal/wiki/Agy-Architecture`
     - Motor & Ferramentas: `https://github.com/julioCoronetti/frontal/wiki/Agy-Backend`
     - Interface & TUI: `https://github.com/julioCoronetti/frontal/wiki/Agy-UI`
   - **Dossiê OpenCode:**
     - Visão Geral: `https://github.com/julioCoronetti/frontal/wiki/OpenCode`
     - Arquitetura & Monorepo: `https://github.com/julioCoronetti/frontal/wiki/OpenCode-Architecture`
     - Motor & Streaming: `https://github.com/julioCoronetti/frontal/wiki/OpenCode-Backend`
     - Interface & TUI: `https://github.com/julioCoronetti/frontal/wiki/OpenCode-UI`
   - **Planejamento e Roadmap (`Board-Roadmap`):**
     - Página: `https://github.com/julioCoronetti/frontal/wiki/Board-Roadmap`
     - *Diretriz de Uso:* O conteúdo do `Board-Roadmap` documenta a proposta conceitual derivada da pesquisa técnica. A especificação formal consolidada encontra-se no `README.md` e o código funcional encontra-se 100% entregue nas Fases 1 a 4 em `main`.
   - **Histórico do Legado:**
     - Página: `https://github.com/julioCoronetti/frontal/wiki/Legacy` (protótipo Python/Textual preservado na branch `origin/legacy`).

---

## 5. Fluxo de Entrega e Governança Git (Padrão de Commits Atômicos)

### Esteira Obrigatória de 5 Gates
Para cada issue, correção ou funcionalidade, executar obrigatoriamente a esteira de validação antes de qualquer commit:
1. `cargo fmt --check`
2. `cargo check --all-targets`
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo test` (validando a integridade dos 83 testes no binário unificado `tests/suite.rs`)
5. Verificação mecânica de ausência total de comentários no código:
   `! grep -rnE '//|/\*' src/ tests/`

### Disciplina de Commits Atômicos (Fim dos "Phase Dumps")
1. **1 Commit = 1 Mudança Lógica Indivisível:** Cada commit deve introduzir uma única alteração atômica que compila e passa nos 5 gates da esteira. Nunca misturar na mesma árvore refatoração estrutural, correção de bug e documentação.
2. **Limite Operacional:** Máximo sugerido de **3 a 5 arquivos alterados por commit** (salvo renomeações estruturais puras via `git mv`).
3. **Registro Arquitetural Permanente:** Como comentários no código são estritamente proibidos, o corpo da mensagem de commit é obrigatório para mudanças não-triviais e atua como a memória histórica do projeto (`git blame`).

### Template Canônico de Commit
```text
<type>(<scope>): <imperative summary in english, max 72 chars>

- Why: <justificativa técnica ou causa raiz do problema>
- What: <síntese objetiva das alterações estruturais ou comportamentais>
- Invariants: <invariantes preservadas: cache L1/L2, swap App/TabState, pgid POSIX, etc.>

Refs: #<card_ou_issue>
Card-Status: <In Progress | In Review | Done>
Reviewed-By: Argon-Advisor <mode:none|review|design|troubleshoot|deep-dive>
```
- **Tipos Permitidos:** `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`.
- **Escopos Padronizados:** `config`, `protocol`, `transport`, `engine`, `session`, `cli`, `ui/app`, `ui/cache`, `ui/editor`, `ui/markdown`, `ui/modal`, `ui/tab`, `ui/viewport`, `tests`, `agents`, `wiki`.

---

## 6. Metodologia de Desenvolvimento Agêntica e Workflow de Cards

### Ciclo de Fatias Verticais Atômicas (Atomic Vertical Slice TDD)
Cada demanda do Board é executada em ciclos mínimos de fatias verticais seguindo 4 passos:
1. **Passo 1 (Spec & Red Test):** O agente Flash escreve o teste no domínio correspondente em `tests/domains/` comprovando a especificação ou reproduzindo o bug. Comandos de validação confirmam a falha esperada.
2. **Passo 2 (Green Implementation):** O agente Flash implementa o código mínimo em `src/`, respeitando as invariantes, o teto de 500 linhas por arquivo e a regra de zero comentários.
3. **Passo 3 (Gate Pipeline & Atomic Commit):** O agente Flash executa os 5 gates de validação e realiza o commit atômico imediatamente com o template canônico.
4. **Passo 4 (Adversarial Review):** Concluídas as fatias da funcionalidade, o card entra em `In Review` para validação com perspectiva adversarial.

### Governança Triádica (Senhor ↔ Flash ↔ Argon)
- **Senhor (Product Owner & Arquiteto-Chefe):** Define prioridades do Board, aprova mudanças de dependências ou escopo, avalia comportamentos e detém autorização exclusiva para mover cards para `Done`.
- **Flash (Lead Agent & Executor Autônomo):** Executa fatias verticais, escreve testes e código, cumpre os limites de modularidade (< 500 linhas) e mantém a esteira verde.
- **Argon (Auditor Estratégico Consultivo):** Auditor sênior read-only acionado estritamente nos 4 gatilhos normativos (`advisor.sh` nos modos `review`, `design`, `troubleshoot` ou `deep-dive`).

### Ciclo de Vida dos Cards no Project Board
1. **`Todo` -> `In Progress`:** Retirar o card prioritário do Backlog e iniciar a implementação em fatias atômicas.
2. **`In Progress` -> `In Review`:** Concluída a fatia e aprovada na esteira de 5 gates, o card move-se para `In Review`. Nunca mover diretamente para `Done`.
3. **Ciclo de Testes Adversariais (`In Review`):** O agente Flash reexecuta testes adotando intencionalmente perspectivas diferentes (cenários adversariais, concorrência, limites de buffers). Havendo correções, realizá-las em commits atômicos permanecendo em `In Review`.
4. **Transição para `Done`:** Quando nenhum teste falhar e nenhuma alteração restar pendente, emitir o aviso:
   > *"Revisão completa. Nenhuma alteração pendente."*
   O card só pode ser movido para `Done` após validação e autorização explícita do Senhor.
