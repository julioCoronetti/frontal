# Diretrizes do Agente - Repositório Frontal

## 1. Visão Geral do Projeto
- **Projeto**: Frontal.
- **Objetivo**: Interface de usuário em terminal (TUI) desacoplada, de alta performance e assíncrona para o Antigravity CLI (`agy`) e motores compatíveis com o protocolo `stream-json`.
- **Status do Projeto**: Arquitetura em Rust implementada e operacional (versão `0.7.1`). As Fases 1 a 4 do roadmap foram integralmente concluídas na branch `main` e consolidadas por suíte unificada de integração com 8 domínios (119 testes). A Fase 5 (polimento de UI/UX, ergonomia e alinhamento visual com OpenCode) encontra-se em desenvolvimento ativo.
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
   - O Nível 1 preserva a AST semântica imutável (`MessageBlock` com spans de estilo em `src/ui/cache/`) para turnos consolidados.
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
- `src/ui/app/`: Módulo central de aplicação particionado em subarquivos coesos (< 500 linhas):
  - `src/ui/app/mod.rs`: Struct `App`, enum `AppAction`, construtores (`new`, `new_isolated`) e getters de metadados ativos.
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
- `src/ui/editor/`: Editor de prompt multilinha sobre B-Tree (`ropey`), extmarks atômicos (`@`, `/`) e pílulas de paste (`mod.rs`, `mutations.rs`, `navigation.rs`).
- `src/ui/autocomplete.rs`: Popover flutuante de autocompletação para comandos e arquivos (`FileSearchWorker`).
- `src/ui/history.rs`: Persistência de histórico de comandos em SQLite WAL (`PromptHistory`).
- `src/ui/modal/`: Pilha de diálogos modais flutuantes particionada (`mod.rs`, `diff.rs`, `model_effort.rs`, `palette.rs`, `respawn.rs`, `select.rs`).
- `src/ui/diff.rs`: Visualizador de diffs unificados integrado.
- `src/ui/clipboard.rs`: Suporte à área de transferência do sistema via `arboard` com buffer isolado em memória e fallback OSC 52.
- `src/ui/notify.rs`: Notificações sensoriais de terminal (campainha auditiva e flashes visuais).
- `src/ui/terminal.rs`: Inicialização de modo raw, bracketed paste e panic hook para restauração de TTY.
- `src/ui/theme.rs`: Definições visuais e paleta de cores inspirada no OpenCode.

### Suíte Unificada de Testes de Integração (`tests/`)
- `tests/suite.rs`: Ponto de entrada canônico do binário único de testes (*single integration test crate*).
- `tests/common/mod.rs`: Utilitários compartilhados herméticos (`test_app`, `press_key`, guards RAII `TempDirGuard`/`TempFileGuard` com gerador atômico de sequência, `PGID_TEST_LOCK`).
- `tests/domains/architecture.rs`: Testes de invariantes mecânicas de arquitetura (teto de 500 linhas, ausência de comentários, matriz adversarial e sincronização de metadados, 4 testes).
- `tests/domains/protocol_and_cli.rs`: Testes de protocolo `stream-json`, NDJSON, `LinesCodec` (16 MB), flags CLI e `--version` (13 testes).
- `tests/domains/engine_and_session.rs`: Testes de subprocesso `agy`, replay de fixtures, POSIX `killpg`/`setpgid` e multiplexação MPSC (10 testes).
- `tests/domains/cache_and_markdown.rs`: Testes de RenderCache L1/L2 $O(1)$, streaming Markdown, syntax highlighting `syntect`, tool cards e 60 FPS (16 testes).
- `tests/domains/viewport_and_clipboard.rs`: Testes de sticky scroll, seleção limpa, word-wrap UTF-8, OSC 52, notificações sensoriais e PromptEditor (15 testes).
- `tests/domains/editor_and_history.rs`: Testes de editor `ropey`, extmarks atômicos, paste pills, autocomplete popover e persistência SQLite WAL (16 testes).
- `tests/domains/tabs_and_modals/`: Testes particionados de multissessão e diálogos (< 500 linhas por arquivo, 29 testes):
  - `tabs.rs`: Ciclo de vida de abas, streaming em segundo plano, contagem de tokens e isolamento de estado.
  - `modals.rs`: Pilha de modais, paleta de comandos, diff viewer integrado, hot-respawn e backdrop dimming overlay.
  - `model_effort.rs`: Seletor de modelo/esforço, normalização de catálogo e restauração de seleções.
  - `select.rs`: Componente genérico DialogSelect, filtragem multitermo, navegação por teclado, scroll offset e viewport clipping.
- `tests/domains/tui_integration/`: Testes particionados fim-a-fim (< 500 linhas por arquivo, 16 testes):
  - `rendering.rs`: Layout visual OpenCode, spinner, cabeçalho/rodapé e dimensões adversariais de terminal.
  - `input_and_events.rs`: Comandos locais (`/quit`, `/clear`, `/help`, etc.), mouse routing e bracketed paste.

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
   - **Arquitetura Implementada (Rust):** `https://github.com/julioCoronetti/frontal/wiki/Frontal-Architecture` (espelho local em `.wiki/Frontal-Architecture.md`)
   - **Espelho Local da Wiki (`.wiki/`):** O repositório `frontal.wiki.git` reside mapeado localmente no diretório `.wiki/` (ignorado no `.gitignore`). Alterações de arquitetura, novos comandos CLI/TUI ou fechamentos de fase devem atualizar a documentação em `.wiki/*.md` no mesmo ciclo de entrega do card.
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

### Esteira Mecânica Nativa de Qualidade (Tríade Cargo)
Para cada issue, correção ou funcionalidade, a validação é executada nativamente pelo Cargo e assegurada pelo hook `.githooks/pre-commit` antes de qualquer commit:
1. `cargo fmt --check`: Formatação canônica do código.
2. `cargo clippy --all-targets -- -D warnings`: Compilação e linting estrito sem avisos (*zero warnings*).
3. `cargo test --quiet`: Execução da suíte unificada de integração em `tests/suite.rs` (validando as regras de arquitetura em `architecture.rs` e os 8 domínios funcionais, 119 testes).

### Ativação Obrigatória dos Git Hooks
O repositório possui hooks mecânicos versionados em `.githooks/`. Todo ambiente de desenvolvimento deve ativá-los obrigatoriamente:
```bash
git config core.hooksPath .githooks
```

### Política de Versionamento SemVer
1. **Série `0.y.z`:** O projeto segue SemVer para softwares em maturação contínua. A versão `0.7.1` reflete a conclusão das Fases 1 a 4 e o ciclo ativo da Fase 5 (Ergonomia e Alinhamento OpenCode).
2. **Regra de Ouro SemVer:** É terminantemente proibido alterar o campo `version` no `Cargo.toml` em commits intermediários (`Card-Status: In Progress`).
3. **Bump Exclusivo no Fechamento (`Done`):** O incremento SemVer ocorre exclusivamente no commit de encerramento (`Card-Status: Done`) autorizado pelo Senhor:
   - **Patch (`0.7.x`):** Para correções de bugs (`fix(...)`) ou refatorações puras.
   - **Minor (`0.x.0`):** Para fechamento de cards funcionais de features estruturais do Board.
   - O commit de fechamento deve ser acompanhado imediatamente pela criação da tag Git anotada (`git tag -a vX.Y.Z -m "Release vX.Y.Z"`).
4. **Simetria de Exposição:** A versão da aplicação deve ser exposta de forma idêntica tanto via CLI (`frontal --version` / `frontal -V`) quanto na TUI (no cabeçalho de resposta do comando `/help`).

### Disciplina de Commits Atômicos (Fim dos "Phase Dumps")
1. **1 Commit = 1 Mudança Lógica Indivisível:** Cada commit deve introduzir uma única alteração atômica que compila e passa na esteira de 6 gates. Nunca misturar na mesma árvore refatoração estrutural, correção de bug e documentação.
2. **Limite Operacional:** Máximo estrito de **3 a 5 arquivos alterados por commit** (salvo renomeações estruturais puras via `git mv` ou flag `ATOMIC_OVERRIDE=1`).
3. **Registro Arquitetural Permanente:** Como comentários no código são estritamente proibidos, o corpo da mensagem de commit é obrigatório para mudanças não-triviais e atua como a memória histórica do projeto (`git blame`).

### Template Canônico de Commit
```text
<type>(<scope>): <imperative summary in english, max 72 chars>

- Why: <justificativa técnica ou causa raiz do problema>
- What: <síntese objetiva das alterações estruturais ou comportamentais>
- Invariants: <invariantes preservadas: cache L1/L2, swap App/TabState, pgid POSIX, etc.>

Refs: #<card_ou_issue>
Card-Status: <In Progress | In Review | Done>
Reviewed-By: Argon-Advisor <mode:none|review|design|troubleshoot|deep-dive|refine>
```
- **Tipos Permitidos:** `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`.
- **Escopos Padronizados:** `config`, `protocol`, `transport`, `engine`, `session`, `cli`, `ui/app`, `ui/cache`, `ui/editor`, `ui/markdown`, `ui/modal`, `ui/tab`, `ui/viewport`, `tests`, `agents`, `wiki`.
- **Trava Mecânica do Advisor (`commit-msg`):** Se `Card-Status: In Review` for declarado, o hook rejeita o commit caso `Reviewed-By:` contenha `<mode:none>`. A transição para revisão exige auditoria prévia obrigatória com o Argon Advisor.

---

## 6. Metodologia de Desenvolvimento Agêntica e Workflow de Cards

### Ciclo Triádico por Card (Senhor ↔ Flash ↔ Argon)
Cada demanda do Board é executada em um ciclo estruturado de 4 fases:

1. **Fase 1 (Design & Action Plan - Kickoff):**
   - Ao mover o card de `Todo` para `In Progress`, para demandas arquiteturais ou de feature, o agente Flash invoca preventivamente o Advisor:
     `~/.gemini/config/scripts/advisor.sh --mode design -f <arquivos_afetados> "Plano de acao para o card #N: <titulo>"`
   - O parecer do Argon antecipa premissas ocultas, blind spots, complexidade ciclomática e riscos de regressão antes de qualquer alteração no código.

2. **Fase 2 (Execução TDD em Fatias Atômicas):**
   - O agente Flash implementa em ciclos mínimos de TDD (Teste Vermelho -> Código Verde -> `cargo test` -> Commit Atômico com `Card-Status: In Progress`).
   - Cada fatia cumpre o teto de 500 linhas por arquivo e a regra de zero comentários.

3. **Fase 3 (Auditoria Adversarial & Trava de Revisão):**
   - Concluídas todas as fatias do card, o agente Flash executa a auditoria adversarial completa via diff:
     `git diff HEAD~N | ~/.gemini/config/scripts/advisor.sh --mode review "Auditoria de fechamento do card #N"`
   - Havendo parecer favorável, o commit de revisão é registrado com `Card-Status: In Review` e `Reviewed-By: Argon-Advisor <mode:review>`.
   - O agente Flash emite o aviso formal ao Senhor:
     > *"Revisão completa. Nenhuma alteração pendente."*

4. **Fase 4 (Fechamento do Card & SemVer):**
   - O Senhor avalia e concede a autorização explícita para `Done`.
   - O agente Flash executa o bump SemVer no `Cargo.toml` (se aplicável), registra o commit final `Card-Status: Done`, cria a tag Git anotada `vX.Y.Z` e atualiza o status do card no Project Board #8 via GitHub CLI:
     `gh project item-edit --id <ITEM_ID> --project-id PVT_kwHOC8mDy84Blf4U --field-id PVTSSF_lAHOC8mDy84Blf4UzhkMioY --single-select-option-id 98236657`
