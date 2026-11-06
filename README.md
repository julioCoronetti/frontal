# Frontal

Terminal User Interface (TUI) harness desacoplado, assíncrono e de alta performance desenvolvido em **Rust** (versão `0.8.0`) para o Antigravity CLI (`agy`) e motores agênticos compatíveis com o protocolo `stream-json`.

---

## 1. Visão Geral

O Frontal provê uma camada de apresentação em terminal de altíssima performance focada na renderização estável e responsiva de fluxos contínuos de tokens de LLM. O harness consome eventos estruturados (NDJSON) emitidos via pipes de processo (`stdin`/`stdout`), desacoplando integralmente o ciclo de vida do motor de IA da interface visual. A estabilidade mecânica da arquitetura é validada por uma suíte unificada de **129 testes automatizados em 8 domínios**.

O design arquitetural é fundamentado na engenharia reversa e análise comparativa de dois harnesses de referência:
- **Antigravity CLI (`agy`)**: Referência em simplicidade operacional, estabilidade de processos e protocolo enxuto `stream-json`.
- **OpenCode**: Referência em ergonomia visual de terminal, decomposição de mensagens em partes atômicas, cards de ferramentas e controle de contexto.

---

## 2. Stack Tecnológica Oficial (Rust)

A arquitetura do Frontal foi definida para garantir execução determinística a 60 FPS com consumo mínimo de recursos e zero dependências externas de runtime:

| Camada | Crate / Biblioteca | Papel e Justificativa Técnica |
| :--- | :--- | :--- |
| **Apresentação & TUI** | `ratatui` + `crossterm` | Renderização imediata (*immediate-mode*) com double-buffering e diff de células em microssegundos. |
| **Async Runtime & IPC** | `tokio` + `tokio-util` | Orquestração assíncrona de subprocessos (`tokio::process`), canais MPSC e decodificação de linhas NDJSON via `LinesCodec`. |
| **Serialização** | `serde` + `serde_json` | Deserialização estrita e tipada dos eventos e comandos do protocolo `stream-json`. |
| **Editor de Prompt** | `ropey` + Token Registry | Buffer baseado em B-Tree (Rope) para navegação multilinha rápida, suporte a menções atômicas (`@`, `/`) e colapso de textos colados longos (`[Pasted ~N lines]`). |
| **Parsing de Markdown** | `pulldown-cmark` | Parser de eventos CommonMark de alto desempenho gerando árvore interna de blocos modulares (`MessageBlock`). |
| **Realce de Sintaxe** | `syntect` | Destaque sintático incremental por linha com 150+ gramáticas embutidas sem necessidade de compilação de código C nativo. |
| **Persistência Local** | `rusqlite` (WAL mode) | SQLite embutido leve para metadados de sessão, histórico de comandos e cache. |
| **Área de Transferência** | `arboard` + OSC 52 | Suporte nativo ao clipboard do sistema com fallback para escape sequences OSC 52 em sessões remotas (SSH). |
| **Isolamento de Processo & POSIX** | `libc` | Grupos de processo POSIX (`setpgid` / `killpg`) e hook de terminação de processos órfãos (`PR_SET_PDEATHSIG`). |
| **Buffers & Utilitários Async** | `bytes` + `futures-util` | Gerenciamento de buffers de rede e manipuladores de stream assíncrono com alocação mínima. |
| **Terminal Virtual (PTY)** | `portable-pty` (RFC futuro, fora do `Cargo.toml`) | Pseudo-terminais assíncronos multiplataforma para comandos interativos (extensão opcional futura). |

---

## 3. Diretrizes de Arquitetura

1. **Cache de Renderização em Dois Níveis**:
   - Mensagens de turnos consolidados são preservadas como AST semântica imutável (`MessageBlock` com spans destacados) no Nível 1.
   - O Nível 2 mantém linhas visuais pré-formatadas indexadas por largura e revisão `(width, revision)`, invalidado preguiçosamente apenas em redimensionamentos (`SIGWINCH`) ou expansão/colapso de blocos.
   - Apenas o turno ativo em streaming processa deltas, mantendo o custo de desenho em $O(1)$ e a taxa de quadros estável em 60 FPS independentemente do tamanho da conversa.
2. **Sticky Scroll Inteligente**:
   - Viewport travado na borda inferior durante o streaming de tokens, com desancoragem imediata ao navegar para cima e reancoragem ao retornar ao rodapé.
3. **Colapso de Pastes Volumosos**:
   - Inserções da área de transferência com mais de 5 linhas são compactadas em pílulas visuais no prompt, evitando quebras de layout.
4. **Isolamento e Limpeza de Subprocessos**:
   - Configuração de `PR_SET_PDEATHSIG` (Linux) e grupos de processo (`setpgid` / `killpg`) para garantir que encerramentos do Frontal nunca deixem processos órfãos do `agy` em execução.

---

## 4. Capacidades Arquiteturais Consolidadas e Evolução Contínua

A arquitetura do Frontal encontra-se implementada e operacional (versão `0.8.0`), validada por **129 testes automatizados em 8 domínios**. O desenvolvimento opera em fluxo contínuo orientado por demandas e cards no GitHub Project Board #8, estruturado nos seguintes domínios consolidados:

- **Fundação, Protocolo `stream-json` e IPC Headless:**
  - Workspace Cargo, multiplexação interna via `SessionId`, tipagem `stream-json`, abstração `EngineTransport` (`SubprocessEngine` e `FixtureReplayEngine`), orquestração assíncrona com `LinesCodec` (16 MB) e isolamento POSIX robusto (`setpgid`/`killpg`/`PR_SET_PDEATHSIG`).
- **Core TUI (60 FPS), Viewport Sticky Scroll e Cache em 2 Níveis:**
  - Loop assíncrono Ratatui a 60 FPS debounced, *bracketed paste*, *panic hook* de restauração de TTY, sticky scroll determinístico e cache de 2 níveis (AST semântica imutável no Nível 1 e linhas indexadas por `(width, revision)` no Nível 2).
- **Streaming Incremental de Markdown, Realce `syntect` e Tool Cards:**
  - Pipeline incremental `pulldown-cmark` resiliente a cercas incompletas, realce `syntect` com checkpoints de estado por linha e cards dinâmicos colapsáveis para chamadas de ferramentas e raciocínio.
- **Editor `ropey` com Extmarks Atômicos, Autocomplete e Persistência SQLite WAL:**
  - Buffer de edição multilinha sobre `ropey`, primitiva de overlay/popover com foco prioritário, extmarks atômicos para menções (`@`, `/`), colapso de pastes volumosos (`HashMap` externo), busca assíncrona de arquivos e histórico persistente em SQLite WAL (`rusqlite`).
- **Multissessão (`TabState`), Pilha de Modais (`DialogSelect`), Guardião de Segurança e Hot-Respawn:**
  - Multissessão (`Alt+1..9`) sobre `SessionManager`, pilha de diálogos modais flutuantes com *backdrop dimming*, Guardião de Segurança determinístico (`DangerAssessment`) com modal de contenção e confirmação (`DangerConfirmationModal`) para interceptação de comandos destrutivos, componente unificado `DialogSelect`, alternância de modelos via *hot-respawn*, paleta de comandos (`Ctrl+P`) e visualizador de diffs integrado.
- **Ergonomia Visual OpenCode, Tema Atlas Crimson e Fidelidade de Clipboard:**
  - Estado de boas-vindas centralizado, transição fluida para layout de duas colunas (chat + Context Sidebar), popover de autocompletação em duas colunas alinhadas com janela deslizante, indicador flutuante de rolagem, tema visual **Atlas Crimson**, seletor `/model` normalizado com *slider* horizontal de esforço (`◀ ●──◎──● ▶`), seleção granular de texto no viewport e cópia fiel para a área de transferência.

---

## 5. Compilação, Testes e Execução

### Pré-requisitos
- Toolchain Rust estável (`cargo`, `rustc` 2021 edition).
- Subprocesso `agy` ou motor compatível acessível no `PATH` (ou configurado via `--agy-bin`).

### Comandos de Build e Teste
```bash
# Compilação otimizada para release
cargo build --release

# Validação integral da suíte unificada de 129 testes em 8 domínios
cargo test --quiet
```

### Inicialização e Opções CLI
```bash
# Inicialização interativa padrão (TUI)
cargo run --release

# Especificação de modelo, esforço de raciocínio e modo de execução
cargo run --release -- --model gemini-3.8-flash-high --effort high --mode plan

# Retomada de conversa existente por UUID
cargo run --release -- --conversation <UUID>

# Execução headless automatizada com prompt único
cargo run --release -- --headless -p "Descreva a arquitetura do projeto"
```

---

## 6. Documentação e Especificações Técnicas

A documentação técnica oficial reside na [Wiki do GitHub](https://github.com/julioCoronetti/frontal/wiki):

- **[Home](https://github.com/julioCoronetti/frontal/wiki/Home)**: Hub central de documentação.
- **[Frontal-Architecture](https://github.com/julioCoronetti/frontal/wiki/Frontal-Architecture)**: Dossiê oficial da arquitetura implementada em Rust (v0.8.0), módulos, tema Atlas Crimson e suíte de 129 testes.
- **[Board-Roadmap](https://github.com/julioCoronetti/frontal/wiki/Board-Roadmap)**: Especificação completa da stack em Rust, diretrizes arquiteturais, capacidades consolidadas e evolução contínua.
- **Dossiê Antigravity CLI (`agy`)**:
  - [Agy](https://github.com/julioCoronetti/frontal/wiki/Agy): Visão geral do CLI oficial.
  - [Agy-Architecture](https://github.com/julioCoronetti/frontal/wiki/Agy-Architecture): Binário Go, orquestração e protocolo `stream-json`.
  - [Agy-Backend](https://github.com/julioCoronetti/frontal/wiki/Agy-Backend): Motor Cortex, ferramentas, segurança e persistência SQLite.
  - [Agy-UI](https://github.com/julioCoronetti/frontal/wiki/Agy-UI): Bubble Tea v2, 48 modelos, EditArea e viewports.
- **Dossiê OpenCode**:
  - [OpenCode](https://github.com/julioCoronetti/frontal/wiki/OpenCode): Visão geral da referência ergonômica.
  - [OpenCode-Architecture](https://github.com/julioCoronetti/frontal/wiki/OpenCode-Architecture): Monorepo de 34 pacotes, runtime Effect-TS e bi-processo RPC.
  - [OpenCode-Backend](https://github.com/julioCoronetti/frontal/wiki/OpenCode-Backend): Ciclo de turnos, bounding de ferramentas, Git sombra e compactação.
  - [OpenCode-UI](https://github.com/julioCoronetti/frontal/wiki/OpenCode-UI): SolidJS + OpenTUI, Tree-sitter WASM, extmarks, keymaps e theming.
- **[Legacy](https://github.com/julioCoronetti/frontal/wiki/Legacy)**: Registro técnico do protótipo inicial v0.1 em Python / Textual (código integral preservado na branch [`legacy`](https://github.com/julioCoronetti/frontal/tree/legacy)).
