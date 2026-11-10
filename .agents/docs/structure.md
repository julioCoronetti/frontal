# Mapeamento Estrutural do Repositório Frontal

Documentação de referência para consulta sob demanda dos módulos do repositório.

## Núcleo & IPC (`src/`)
- `src/main.rs`: Ponto de entrada do executável da aplicação.
- `src/lib.rs`: Declaração dos módulos da biblioteca pública (`cli`, `config`, `engine`, `protocol`, `session`, `transport`, `ui`).
- `src/config.rs`: Configuração persistente do usuário (`~/.frontal/config.toml`, modelo, modo e esforço padrão).
- `src/cli.rs`: Parser de argumentos de linha de comando (`CliArgs`), orquestração do modo headless e inicialização do TUI.
- `src/protocol.rs`: Tipagem estrita e desserialização NDJSON do protocolo `stream-json` (`EngineEvent`, `StreamEvent`, `ToolInfo`, etc.).
- `src/transport.rs`: Implementação de `EngineTransport` (`SubprocessEngine` com buffer `LinesCodec` de 16 MB e `FixtureReplayEngine`).
- `src/engine.rs`: Abstração de alto nível do motor agêntico e canais de streaming.
- `src/session.rs`: Gerenciamento e multiplexação de sessões via `SessionId` e `SessionManager`.
- `src/security.rs`: Guardião de contenção determinístico de segurança (`DangerAssessment`, `RiskCategory`) para inspeção de chamadas destrutivas.

## Interface de Terminal (`src/ui/`)
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
- `src/ui/modal/`: Pilha de diálogos modais flutuantes particionada (`mod.rs`, `danger.rs`, `diff.rs`, `model_effort.rs`, `palette.rs`, `respawn.rs`, `select.rs`, `usage/`).
- `src/ui/diff.rs`: Visualizador de diffs unificados integrado.
- `src/ui/clipboard.rs`: Suporte à área de transferência do sistema via `arboard` com buffer isolado em memória e fallback OSC 52.
- `src/ui/notify.rs`: Notificações sensoriais de terminal (campainha auditiva e flashes visuais).
- `src/ui/terminal.rs`: Inicialização de modo raw, bracketed paste e panic hook para restauração de TTY.
- `src/ui/theme.rs`: Definições visuais e paleta de cores inspirada no OpenCode.
