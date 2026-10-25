# Frontal

Terminal User Interface (TUI) harness desacoplado, assíncrono e de alta performance desenvolvido em **Rust** para o Antigravity CLI (`agy`) e motores agênticos compatíveis com o protocolo `stream-json`.

---

## 1. Visão Geral

O Frontal provê uma camada de apresentação em terminal de altíssima performance focada na renderização estável e responsiva de fluxos contínuos de tokens de LLM. O harness consome eventos estruturados (NDJSON) emitidos via pipes de processo (`stdin`/`stdout`), desacoplando integralmente o ciclo de vida do motor de IA da interface visual.

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
| **Terminal Virtual (PTY)** | `portable-pty` | Pseudo-terminais assíncronos multiplataforma para execução de comandos interativos (extensão opcional futura). |

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

## 4. Roadmap de Implementação

- **Fase 1: Fundação, Protocolo e IPC Headless (MVP Engine)**
  - Workspace Cargo, tipagem do protocolo `stream-json`, orquestração assíncrona do subprocesso `agy` e tratamento de sinais POSIX.
- **Fase 2: Viewport da Conversa e Streaming Reativo**
  - Loop de renderização Ratatui a 60 FPS debounced, viewport com sticky scroll, streaming de Markdown e cards visuais de ferramentas.
- **Fase 3: Editor de Prompt com Tokens Semânticos e Autocomplete**
  - Buffer de texto sobre `ropey`, suporte a extmarks para menções (`@`, `/`), paste collapse e histórico persistente em SQLite.
- **Fase 4: Abas Multissessão, Diálogos Modais e Ergonomia Avançada**
  - Gerenciamento de múltiplas sessões (`Alt+1..9`), pilha de diálogos modais flutuantes, paleta de comandos (`Ctrl+P`) e visualizador de diffs.

---

## 5. Documentação e Especificações Técnicas

A documentação detalhada de engenharia reversa e arquitetura reside na [Wiki Oficial do GitHub](https://github.com/julioCoronetti/frontal/wiki):

- **[[Home]](https://github.com/julioCoronetti/frontal/wiki/Home)**: Hub central de documentação.
- **[[Board-Roadmap]](https://github.com/julioCoronetti/frontal/wiki/Board-Roadmap)**: Especificação completa da stack em Rust, diretrizes arquiteturais e fases.
- **Dossiê Antigravity CLI (`agy`)**:
  - [[Agy-Architecture]](https://github.com/julioCoronetti/frontal/wiki/Agy-Architecture): Binário Go, orquestração e protocolo `stream-json`.
  - [[Agy-Backend]](https://github.com/julioCoronetti/frontal/wiki/Agy-Backend): Motor Cortex, ferramentas, segurança e persistência SQLite.
  - [[Agy-UI]](https://github.com/julioCoronetti/frontal/wiki/Agy-UI): Bubble Tea v2, 48 modelos, EditArea e viewports.
- **Dossiê OpenCode**:
  - [[OpenCode-Architecture]](https://github.com/julioCoronetti/frontal/wiki/OpenCode-Architecture): Monorepo de 34 pacotes, runtime Effect-TS e bi-processo RPC.
  - [[OpenCode-Backend]](https://github.com/julioCoronetti/frontal/wiki/OpenCode-Backend): Ciclo de turnos, bounding de ferramentas, Git sombra, compactação e retentativas.
  - [[OpenCode-UI]](https://github.com/julioCoronetti/frontal/wiki/OpenCode-UI): SolidJS + OpenTUI, Tree-sitter WASM, extmarks, keymaps e theming.
- **[[Legacy]](https://github.com/julioCoronetti/frontal/wiki/Legacy)**: Registro técnico do protótipo inicial v0.1 em Python / Textual (código integral preservado na branch [`legacy`](https://github.com/julioCoronetti/frontal/tree/legacy)).
