# Original User Request

## 2026-10-01T19:56:55Z

This is a single self-contained fix; keep it small and focused.

Desenvolver um frontend TUI desacoplado e de alta performance para o Antigravity CLI (agy) que se comunique com o processo central via protocolo headless stream-json, eliminando quebras artificiais de linha, provendo renderização completa de Markdown e habilitando cópia íntegra de código para a área de transferência do Windows/WSL.

Working directory: /home/julio_regino/workspace/github/agy-tui
Integrity mode: development

## Requirements

### R1. Headless Engine Communication & Concurrency Isolation
Implementar o ciclo de vida do processo `agy` em modo headless (`--input-format stream-json --output-format stream-json`), gerenciando streams bidirecionais assíncronos (stdin/stdout/stderr) sem bloquear a interface gráfica e garantindo isolamento total de sessão para não interferir em outras instâncias do `agy` em execução simultânea.

### R2. TUI Presentation & Markdown Rendering
Renderizar visualmente os fluxos de eventos do agente (mensagens, pensamentos e atualizações de ferramentas) com formatação Markdown estruturada e blocos de código com destaque de sintaxe, sem aplicar quebra de linha artificial (hard-wrap) no conteúdo de código original.

### R3. Clipboard Integration & Code Integrity
Fornecer mecanismo direto de cópia de blocos de código e seleções textuais para a área de transferência do sistema operacional (via `clip.exe` e sequências OSC 52), preservando rigorosamente a indentação original, quebras de linha lógicas e ausência de caracteres espúrios de terminal.

### R4. Interactive Input Handling
Disponibilizar área de entrada de texto com suporte a prompts multilinhas, navegação de histórico de comandos e tratamento de sinais de interrupção (cancelamento da resposta atual sem matar a sessão do motor).

### R5. Network and Environmental Constraints
Operar estritamente local sob ambiente corporativo WSL, sem chamadas externas desnecessárias de rede ou escrita fora do diretório do projeto.

## Verification Resources

- Protocolo de stream documentado do agy:
  - Input (stdin): `{"event": "user", "message": {"content": "<prompt>"}}`
  - Output (stdout): `{"event": "thought"|"step_update"|"message"|"result", ...}`
- Utilitário do sistema para área de transferência: `/mnt/c/Windows/System32/clip.exe`

## Acceptance Criteria

### Stream and Process Management
- [ ] Execução assíncrona do subprocesso `agy` sem travamento da interface durante streaming de tokens.
- [ ] Tratamento gracioso de encerramento e desconexão de processos filhos sem deixar processos zumbis.
- [ ] Não interferência em sessões paralelas pré-existentes do `agy`.

### Rendering and Visual Quality
- [ ] Blocos de código Markdown exibidos com destaque de sintaxe e quebra visual suave (sem injeção de `\n` no buffer textual).
- [ ] Eventos de pensamento (`thought`) e passos de ferramentas (`step_update`) apresentados em seções recolhíveis ou visualmente diferenciadas do output principal.

### Clipboard and Data Integrity
- [ ] Cópia para o clipboard via atalho/botão preserva a indentação e a integridade de sintaxe exatas do bloco copiado.
- [ ] Verificação programática: o conteúdo recuperado do clipboard via script de teste confere byte a byte com o bloco de código original.

### Automation and Testability
- [ ] Suíte de testes automatizados ou script de mock que valida o parser do stream JSON sem requerer interação humana.


## 2026-10-01T21:02:11Z

This is a single self-contained fix; keep it small and focused.

Evoluir o frontend TUI do Antigravity CLI (agy) para uma aplicação de terminal em tela cheia (fullscreen alternate-screen) inspirada na interface do OpenCode, incorporando viewport rolável independente, barra lateral persistente de telemetria/status do agente, input multilinhas fixo no rodapé e paridade completa com os comandos essenciais do agy.

Working directory: /home/julio_regino/workspace/github/agy-tui
Integrity mode: development

## Requirements

### R1. Fullscreen Canvas & OpenCode-Style Layout
Implementar a interface gráfica completa em tela cheia utilizando o framework `textual` (alternate screen buffer), garantindo que a aplicação ocupe 100% da viewport do terminal sem sujar o buffer de histórico ao sair. O layout deve conter:
- **Painel Central de Mensagens:** Viewport com rolagem vertical independente (suporte à roda do mouse e teclas PgUp/PgDn), autoscroll ao receber novos tokens de streaming e renderização de Markdown fluida.
- **Barra Lateral Persistente (Sidebar Inspector):** Painel lateral retrátil (toggle via `Ctrl+B`) exibindo status do agente (Idle / Thinking / Running Tool), modelo ativo, medidor visual de consumo de tokens/contexto, passos de ferramentas e subagentes ativos.
- **Rodapé Fixo de Entrada:** Área de texto ancorada na base inferior da tela com suporte multilinhas, expansão suave e submissão com `Enter` (ou `Ctrl+Enter` configurável).

### R2. Code Presentation & Interactive Clipboard
Blocos de código devem ser apresentados com destaque sintático de alta fidelidade e botões clicáveis de cópia direta para a área de transferência do Windows/WSL (`clip.exe` / OSC 52). A cópia deve preservar 100% da indentação original, quebras lógicas de linha e integridade dos caracteres, além de manter os atalhos de comando `/copy <id>` e `/copyall`.

### R3. agy Command Parity & Tool Diff Inspection
Incorporar suporte interativo aos comandos e recursos nativos do motor:
- Seleção e troca de modelo (`/model`).
- Configuração de perfil de execução (`/mode`: code, plan, architect, ask) e nível de esforço de raciocínio (`/effort`: low, medium, high).
- Visualizador de diffs com destaque para modificações de arquivo geradas pelas ferramentas de escrita.
- Sugestão e menu de autocomplete navegável ao digitar `/`.
- Gestão e listagem de histórico de sessões (`/resume`).

### R4. Strict Directory Containment & Concurrency
Todas as modificações de código, arquivos auxiliares, testes e caches devem residir estritamente no diretório do projeto (`/home/julio_regino/workspace/github/agy-tui`). Proibida a criação de arquivos fora desse escopo. O motor assíncrono deve manter isolamento estrito de sessão por UUID para não interferir em outras instâncias concorrentes do `agy`.

## Verification Resources

- Framework instalado no ambiente: `textual 8.2.8` (`python3 -c "import textual"`)
- Protocolo stream-json do agy já implementado em `agy_tui/protocol.py` e `agy_tui/engine.py`
- Executável de clipboard do host: `/mnt/c/Windows/System32/clip.exe`

## Acceptance Criteria

### Layout and Responsiveness
- [ ] A aplicação inicia em tela cheia (alternate screen) e restaura perfeitamente o terminal no encerramento.
- [ ] A rolagem do histórico de mensagens funciona via roda do mouse e teclado sem mover o campo de input nem o cabeçalho.
- [ ] A barra lateral (Sidebar) exibe corretamente as informações em tempo real e pode ser recolhida/expandida via tecla de atalho (`Ctrl+B`).

### Interactive Features and Clipboard
- [ ] Cópia via clique no botão do bloco ou via comando preserva indentação exata e passa no teste de round-trip com o clipboard do Windows.
- [ ] A área de entrada de texto suporta multilinhas e navegação de histórico de comandos.

### agy Feature Parity
- [ ] Comandos `/model`, `/mode`, `/effort` e visualização de diffs de ferramentas funcionam de forma reativa.
- [ ] Autocomplete ou menu de comandos exibido ao digitar `/`.

### Stability, Code Quality and Containment
- [ ] Zero arquivos criados fora de `/home/julio_regino/workspace/github/agy-tui`.
- [ ] 0 comentários no código-fonte.
- [ ] 100% de aprovação na suíte de testes automatizados (`pytest tests/`) em execução headless.
- [ ] 0 erros de linter (`ruff check agy_tui tests`).
