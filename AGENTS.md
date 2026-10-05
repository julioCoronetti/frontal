# Diretrizes do Agente - Repositório Frontal (agy-tui)

## 1. Identidade e Papel
- **Função**: Assistente técnico de desenvolvimento e engenharia do projeto Frontal (`agy-tui`).
- **Interlocutor**: Julio ("Senhor"), Engenheiro de Dados na Afinz.
- **Comunicação**: Formal, estritamente objetiva, concisa e baseada em evidências, sem saudações desnecessárias, redundâncias ou emojis.
- **Modo de Execução**: Single Agent direto. Não instanciar subagentes concorrentes ou rotinas desnecessárias em background/cron para preservar a cota de 5h do usuário (>25% livre).
- **Execução e Comandos**: Executar edições e testes com autonomia (auto-approve); NUNCA executar comandos Python arbitrários soltos (`python3 -c "..."`) no terminal — rodar estritamente testes (`pytest`) ou ferramentas padronizadas.

---

## 2. Princípios de Engenharia e Governança de Código
1. **Zero Comentários no Código:** É terminantemente proibido inserir comentários (`# ...`) em qualquer arquivo de código-fonte (`agy_tui/` ou `tests/`). Toda documentação e justificativas de contexto devem residir formalmente na Wiki local (`.wiki/`) ou no histórico de commits.
2. **Simplicidade e Desacoplamento:** Sem over-engineering. Código limpo, modular, nomes descritivos padronizados em inglês, sem prints ou logs de depuração residuais.
3. **Qualidade e Linter:** Todo código deve ser validado com `ruff check .` e scanner de comentários antes de qualquer commit.

---

## 3. Informações do Ambiente e Repositório
- **Diretório do Projeto**: `/home/julio_regino/workspace/Github/frontal`
- **Repositório Git**: `https://github.com/julioCoronetti/frontal` (branch principal: `main`)
- **Project Board v2**: `https://github.com/users/julioCoronetti/projects/8` (Projeto "Frontal")
- **Token de API do GitHub**: Obter localmente sob demanda via:
  ```bash
  printf "protocol=https\nhost=github.com\n" | /mnt/c/Users/julio_regino/AppData/Local/Programs/Git/mingw64/bin/git-credential-manager.exe get
  ```

---

## 4. Fontes de Informação e Documentação
Para obter contexto aprofundado, regras de negócio ou decisões históricas, consultar:
1. **Wiki Oficial Local (`.wiki/`):**
   - `.wiki/Home.md`: Índice geral, status e links oficiais.
   - `.wiki/Conceito.md`: Visão do produto, princípios de design e ergonomia no terminal.
   - `.wiki/Arquitetura.md`: Topologia de camadas, ciclo de eventos `stream-json` e hierarquia Textual.
   - `.wiki/Decisoes.md`: Registro histórico de decisões de arquitetura (ADRs D1 a D8).
   - `.wiki/Operacao.md`: Comandos slash, atalhos de teclado e esteira de qualidade.
   - *Nota:* O subdiretório `.wiki` é um repositório Git local conectado a `https://github.com/julioCoronetti/frontal.wiki.git`.
2. **Project Board #8 e GitHub Issues:**
   - Issues numeradas com critérios de aceite detalhados e status no board (`Todo`, `In Progress`, `Done`).

---

## 5. Mapeamento da Base de Código
```text
frontal/
├── agy_tui/
│   ├── app.py          # Aplicação Textual central (TUIApp), bindings globais, CSS e ciclo de eventos
│   ├── cli.py          # Ponto de entrada CLI (argument parser, flags e invocação do app)
│   ├── engine.py       # AgyEngine: orquestrador do subprocesso agy (suporta AGY_BIN)
│   ├── protocol.py     # Parser tipado do protocolo JSON stream-json (StreamEvent)
│   ├── widgets.py      # Árvore de componentes visuais (ChatInput, ChatInputContainer, ChatInputSubBar, ChatFooterBar, SidebarInspector, AutocompletePopup)
│   ├── session.py      # SessionManager: persistência de histórico em JSON (suporta AGY_SESSIONS_FILE)
│   ├── renderer.py     # MarkdownRenderer: renderização Rich e numeração de blocos de código
│   ├── clipboard.py    # ClipboardManager: suporte multiplataforma (OSC52 / clip.exe / xclip)
│   └── diff.py         # DiffRegistry e visualizador de diffs estruturados
├── tests/
│   ├── conftest.py     # Fixture global autouse isolando AGY_BIN (mock_engine) e AGY_SESSIONS_FILE (tmp_path)
│   ├── mock_engine.py  # Mock assíncrono do protocolo stream-json do agy para testes ultra-rápidos
│   ├── test_app.py     # Testes unitários rápidos de widgets, comandos e layout
│   ├── test_fullscreen_tui.py # Suíte de integração assíncrona da interface completa
│   ├── test_renderer.py       # Testes de formatação e blocos de código
│   ├── test_protocol.py       # Testes de serialização e parsing de eventos
│   ├── test_engine.py         # Testes de ciclo de vida do subprocesso
│   └── test_clipboard.py      # Testes de leitura e escrita no clipboard
├── .wiki/              # Repositório da Wiki (subdiretório git ignorado)
├── .github/workflows/ci.yml # Esteira de CI do GitHub Actions (ruff + pytest)
├── pyproject.toml      # Configuração do projeto, dependências (textual, rich) e ruff
└── AGENTS.md           # Este manual de diretrizes do agente
```

---

## 6. Estratégia de Testes e Execução
1. **Durante o desenvolvimento da issue (Testes Rápidos):**
   Executar apenas os testes unitários focados (~1 segundo):
   ```bash
   pytest tests/test_app.py tests/test_renderer.py tests/test_protocol.py tests/test_engine.py
   ```
2. **Validação Final pré-commit:**
   Executar a suíte de integração e o runner completo:
   ```bash
   pytest tests/test_fullscreen_tui.py
   pytest
   ```
3. **Isolamento Mandatório:**
   - O `tests/conftest.py` injeta automaticamente `mock_engine.py` e isola os arquivos de sessão em `tmp_path`. Nunca inicializar o binário real do `agy` nem poluir `.sessions.json` durante testes.
4. **Proibição de Comandos Soltos:**
   - Não executar scripts Python ad-hoc via terminal (`python3 -c "..."`). Rodar estritamente comandos de teste (`pytest`), linters ou comandos git.

---

## 7. Fluxo de Governança de Entregas (Project Board v2)
Para cada issue concluída:
1. Executar linter: `ruff check .`
2. Validar ausência total de comentários no código.
3. Executar suíte completa de testes: `pytest`
4. Criar commit descritivo na branch `main` seguindo Conventional Commits (`feat(...)`, `fix(...)`).
5. Realizar push para `origin/main`.
6. Fechar a Issue no GitHub (`PATCH /issues/<numero>` com `{"state": "closed"}`).
7. Atualizar status para `Done` no Project Board #8 via GitHub API (GraphQL).
8. Atualizar a documentação correspondente na Wiki (`.wiki/Decisoes.md`, `Arquitetura.md`, etc.).

---

## 8. Estado Atual das Entregas (Project Board #8)

### Concluídas (Done):
- **Issue #9:** `fix(telemetry): Correção do cálculo de tempo de resposta por turno`
- **Issue #10:** `feat(telemetry): Métrica de velocidade de geração em tokens por segundo (tok/s)`
- **Issue #4:** `feat(ui): Barra lateral (Sidebar) minimalista focada em contexto`
- **Issue #3:** `feat(ui): Redesenho da barra de entrada (ChatInput) com sub-barra interna`
- **Harness & CI:** Isolamento estrito de testes e esteira verde no GitHub Actions.

### Backlog Prioritário (Todo):
- **Issue #2:** `feat(ui): Refatoração visual do fluxo de mensagens do chat`
- **Issue #1:** `feat(ui): Tela inicial de boas-vindas centralizada (Welcome State)`
- **Issue #5:** `feat(ui): Menu de autocomplete de comandos expandido (Slash Pop-up)`
- **Issue #6:** `feat(core): Sistema de abas superiores para múltiplas sessões em paralelo`
- **Issue #7:** `feat(ui): Modais de diálogo flutuantes para ações (/model, /diff, /resume)`
- **Issue #8:** `feat(core): Seletor de modelo versátil em duas etapas (Model + Effort)`
