# Diretrizes do Agente - Repositório Frontal

## 1. Visão Geral do Projeto
- **Projeto**: Frontal.
- **Objetivo**: Interface de usuário em terminal (TUI) desacoplada, de alta performance e assíncrona para o Antigravity CLI (`agy`) e motores compatíveis com o protocolo `stream-json`.
- **Status do Projeto**: Arquitetura em Rust implementada e operacional (versão `0.9.0`), consolidada na branch `main` por suíte unificada de integração com 8 domínios (150 testes). O desenvolvimento evolui em fluxo contínuo orientado por cards e issues do GitHub Project Board #8.
- **Stack Oficial Ativa (Crates do `Cargo.toml`)**: `tokio`, `tokio-util`, `bytes`, `futures-util`, `serde`, `serde_json`, `libc`, `ratatui` (0.29), `crossterm` (0.28), `pulldown-cmark` (0.12), `syntect` (5.2), `ropey` (1.6), `rusqlite` (0.32), `arboard` (3.4). Single-crate estrito; sem dependências externas adicionais sem autorização prévia.
- **Branches de Referência**: `main` (ativa) e `origin/legacy` (protótipo Python/Textual preservado).
- **Papel do Agente**: Engenheiro de software focado em features, correções, refatoração e testes com máxima performance, estabilidade e conformidade arquitetural.

---

## 2. Princípios de Engenharia e Invariantes Arquiteturais

### Princípios Fundamentais
1. **Zero Comentários no Código:** É terminantemente proibido inserir comentários no código-fonte. Toda documentação deve residir na Wiki do GitHub, na documentação formal ou nas mensagens de commit.
2. **Simplicidade e Modularidade:** Código limpo, desacoplado, modular e sem over-engineering. Nomes padronizados em inglês. Sem prints residuais, logs ou depuração ad-hoc.
3. **Qualidade e Testes:** Aderência estrita à compilação, linters e testes da stack Rust.
4. **Limite de Complexidade e Modularidade:** Teto formal de **500 linhas** por arquivo `.rs`. Ultrapassando o teto, o módulo deve ser decomposto via blocos `impl` particionados em subdiretórios homônimos (como em `src/ui/app/`), mantendo re-exports públicos e zero quebra de API.

### Invariantes Arquiteturais Obrigatórios
1. **Cache de Renderização em 2 Níveis ($O(1)$ no turno ativo):** Nível 1 preserva a AST imutável (`MessageBlock` com spans de estilo). Nível 2 mantém linhas visuais indexadas por `(width, revision)`. Redimensionamentos ou expansão invalidam apenas Nível 2. Apenas o bloco ativo emite deltas em streaming (60 FPS estável).
2. **Event Loop TUI Não-Bloqueante (60 FPS):** O loop `tokio::select!` em `src/ui/mod.rs` jamais deve executar I/O síncrono ou bloqueante na thread de renderização. Varreduras de disco (`FileSearchWorker`), persistência (`PromptHistory`) e subprocessos operam fora da thread visual.
3. **Sincronização Multisessão (`App` ↔ `TabState` via `std::mem::swap`):** Estado ativo reside em `App` e é permutado via `std::mem::swap`. Novos campos por aba devem obrigatoriamente ser espelhados em `TabState`, incluídos no swap e manipulados em background.
4. **Isolamento POSIX de Subprocessos:** Todo subprocesso agêntico (`agy`) deve ser instanciado em grupo próprio via `setpgid(0, 0)` e registrar `PR_SET_PDEATHSIG(libc::SIGTERM)` no hook `pre_exec`. Encerramento via `killpg` ordenado (`SIGINT` -> `SIGTERM` -> `SIGKILL`).
5. **Isolamento de Efeitos Colaterais em Testes:** Persistência em testes deve usar `PromptHistory::open_in_memory()`. Clipboard em testes deve utilizar `SystemClipboard::new_isolated()`.

---

## 3. Suíte Unificada de Testes de Integração (`tests/`)
A suíte canônica consolidada em `tests/suite.rs` valida hermeticamente todos os domínios do sistema:
- `tests/domains/architecture.rs`: Testes de invariantes mecânicas de arquitetura (4 testes).
- `tests/domains/protocol_and_cli.rs`: Testes de protocolo `stream-json`, NDJSON e CLI (13 testes).
- `tests/domains/engine_and_session.rs`: Testes de subprocesso `agy`, fixtures e POSIX (10 testes).
- `tests/domains/cache_and_markdown.rs`: Testes de RenderCache L1/L2 e Markdown (16 testes).
- `tests/domains/viewport_and_clipboard.rs`: Testes de sticky scroll, seleção limpa e clipboard (16 testes).
- `tests/domains/editor_and_history.rs`: Testes de editor `ropey` e histórico SQLite WAL (16 testes).
- `tests/domains/tabs_and_modals/`: Testes particionados de multissessão e modais (55 testes).
- `tests/domains/tui_integration/`: Testes particionados de integração TUI (20 testes).

*Para consulta estrutural detalhada de módulos `src/` e `src/ui/`, consultar `.agents/docs/structure.md`.*

---

## 4. Recursos Sob Demanda (Progressive Disclosure)
Documentações profundas e procedimentos mecânicos foram desacoplados em módulos especializados sob demanda:
- **Skills Operacionais (`.agents/skills/`):**
  - `frontal-quality-gate`: Execução da tríade Cargo (`cargo fmt`, `cargo clippy`, `cargo test`) e validação de invariantes mecânicas.
  - `frontal-card-lifecycle`: Gestão do ciclo de fechamento de card, SemVer, tags Git anotadas e transição no Board #8.
  - `frontal-wiki-sync`: Auditoria de invariantes, prevenção de tokens extintos e sincronização no espelho local `.wiki/`.
- **Documentação Especializada (`.agents/docs/`):**
  - `.agents/docs/structure.md`: Mapeamento detalhado arquivo por arquivo do núcleo e UI.
  - `.agents/docs/sources.md`: Catálogo completo de links, referências do GitHub, Wiki oficial e dossiês técnicos (`agy`, `OpenCode`).

---

## 5. Fluxo de Entrega e Governança Git (Commits Atômicos)

### Esteira Mecânica Nativa de Qualidade
Para qualquer alteração, a validação canônica é executada via script `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh` e assegurada pelo hook `.githooks/pre-commit`:
1. `cargo fmt --check`: Formatação canônica.
2. `cargo clippy --all-targets -- -D warnings`: Linting rigoroso sem warnings.
3. `cargo test --quiet`: Execução da suíte unificada em `tests/suite.rs` (validando as regras de arquitetura em `architecture.rs` e os 8 domínios funcionais, 150 testes).

### Ativação Obrigatória dos Git Hooks
```bash
git config core.hooksPath .githooks
```

### Política de Versionamento SemVer
1. **Série `0.y.z`:** O projeto segue SemVer para softwares em maturação contínua. A versão `0.9.0` reflete a arquitetura consolidada em produção e a evolução contínua guiada por cards do Project Board.
2. **Regra de Ouro SemVer:** Proibido alterar `version` no `Cargo.toml` em commits intermediários (`Card-Status: In Progress`).
3. **Bump Exclusivo no Fechamento (`Done`):** O incremento ocorre unicamente no commit de encerramento (`Card-Status: Done`) autorizado pelo Senhor:
   - **Patch (`0.9.x`):** Para correções de bugs (`fix(...)`) ou refatorações puras.
   - **Minor (`0.x.0`):** Para fechamento de cards funcionais de features estruturais do Board.
   - O commit de fechamento deve ser acompanhado imediatamente pela criação da tag Git anotada (`git tag -a vX.Y.Z -m "Release vX.Y.Z"`).
4. **Simetria de Exposição:** A versão deve ser idêntica na CLI (`frontal --version` / `frontal -V`) e na TUI (`/help`).

### Disciplina de Commits Atômicos
1. **1 Commit = 1 Mudança Lógica Indivisível:** Nunca misturar refatoração, bug fix e documentação.
2. **Limite Operacional:** Máximo estrito de **3 a 5 arquivos alterados por commit** (salvo renomeações puras ou `ATOMIC_OVERRIDE=1`).
3. **Template Canônico de Commit:**
```text
<type>(<scope>): <imperative summary in english, max 72 chars>

- Why: <justificativa técnica ou causa raiz do problema>
- What: <síntese objetiva das alterações estruturais ou comportamentais>
- Invariants: <invariantes preservadas: cache L1/L2, swap App/TabState, pgid POSIX, etc.>

Refs: #<card_ou_issue>
Card-Status: <In Progress | In Review | Done>
Reviewed-By: Argon-Advisor <mode:none|review|design|troubleshoot|deep-dive|refine>
```
- **Tipos:** `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`.
- **Escopos:** `config`, `protocol`, `transport`, `engine`, `session`, `cli`, `ui/app`, `ui/cache`, `ui/editor`, `ui/markdown`, `ui/modal`, `ui/tab`, `ui/viewport`, `tests`, `agents`, `wiki`.
- **Trava Mecânica do Advisor (`commit-msg`):** Se `Card-Status: In Review` for declarado, o hook rejeita se `Reviewed-By:` contiver `<mode:none>`.

---

## 6. Metodologia de Desenvolvimento Agêntica e Workflow de Cards

### Ciclo Triádico por Card (Senhor ↔ Flash ↔ Argon)
1. **Etapa 1 (Design & Action Plan - Kickoff):** Invocação preventiva do Advisor (`advisor.sh --mode design -f <arquivos> "..."`) ao mover para `In Progress`.
2. **Etapa 2 (Execução TDD em Fatias Atômicas):** Ciclos mínimos de TDD (Teste Vermelho -> Código Verde -> `quality_gate.sh` -> Commit Atômico com `Card-Status: In Progress`).
3. **Etapa 3 (Auditoria Adversarial & Trava de Revisão):** Auditoria de diff pelo Advisor (`advisor.sh --mode review`). Parecer favorável habilita `Card-Status: In Review` e aviso formal ao Senhor.
4. **Etapa 4 (Fechamento do Card & SemVer):** Sob autorização do Senhor, execução do script `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh` para bump SemVer, commit `Card-Status: Done`, tag `vX.Y.Z` e atualização no Board #8.
