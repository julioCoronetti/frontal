# Arquitetura do Frontal (`agy-tui`)

## 1. Princípios de Engenharia
1. **Zero Comentários no Código:** Nenhum comentário (`#`) em código-fonte de produção ou suíte de testes. Todo o contexto e arquitetura residem nesta Wiki.
2. **Desacoplamento Completo:** A TUI (`agy_tui`) é totalmente desacoplada do motor subjacente (`agy`). A comunicação ocorre via pipes assíncronos (`stdin`/`stdout`) com eventos JSON tipados (`StreamEvent`).
3. **Isolamento de Testes:** Todas as execuções de testes utilizam `mock_engine.py` injetado via variável de ambiente `AGY_BIN`, sem acionar processos externos ou poluir arquivos reais de histórico.

---

## 2. Mapa dos Componentes Centrais

### `agy_tui/app.py`
- Ponto central da aplicação Textual (`TUIApp`).
- Gerencia o ciclo de vida da tela, layout principal (área de mensagens e barra lateral retrátil).
- Executa o loop assíncrono de eventos (`_process_events`), despachando deltas de texto, atualizações de ferramentas e finalização de turnos.
- Fornece bindings globais (`Ctrl+B` para sidebar, `Ctrl+M` para modal de modelos, `Ctrl+C` para cópia/interrupção).

### `agy_tui/engine.py`
- Orquestrador de subprocesso (`AgyEngine`).
- Gerencia subprocesso do motor, inicialização via pipes não-bloqueantes e fila assíncrona de eventos (`asyncio.Queue[StreamEvent]`).

### `agy_tui/protocol.py`
- Parser tipado e normalizador do protocolo `stream-json`.
- Converte payloads brutos em instâncias estruturadas de `StreamEvent` (mensagens, passos de ferramentas, status de turnos, telemetria).

### `agy_tui/widgets.py`
- Componentes visuais Textual:
  - `ChatInput`: Área de texto expansível com histórico (`Up`/`Down`), envio (`Enter`), seleção completa (`Ctrl+A`) e suporte a multiline.
  - `ChatInputSubBar`: Sub-barra minimalista com modelo, modo e esforço ativos.
  - `ChatFooterBar`: Rodapé compacto com caminho do workspace e contadores de contexto.
  - `SidebarInspector`: Painel lateral retrátil com telemetria detalhada de tokens, passos e agentes.
  - `ModelSelectModal`: Diálogo flutuante em duas etapas para seleção de modelo e esforço de raciocínio.
  - `MessageCard`: Cartão de mensagem com realce lateral sutil e suporte a clique para cópia.

### `agy_tui/clipboard.py`
- Gerenciador multiplataforma de área de transferência (`ClipboardManager`).
- Suporte ordenado a:
  1. WSL2 via `/mnt/c/Windows/System32/clip.exe` e PowerShell `Get-Clipboard`.
  2. Sequências de escape ANSI OSC 52 para integração direta com emuladores de terminal modernos.
  3. Utilitários Linux (`wl-copy`/`wl-paste` para Wayland, `xclip` para X11).
  4. Buffer de contingência em memória.
