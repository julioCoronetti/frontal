# Diretrizes do Agente - Repositório Frontal (agy-tui)

## 1. Visão Geral do Projeto
- **Projeto**: Frontal (`agy-tui`).
- **Objetivo**: Interface de usuário em terminal (TUI) desacoplada, de alta performance e assíncrona para o Antigravity CLI (`agy`) e motores compatíveis com o protocolo `stream-json`.
- **Stack**: Python 3.10+, Textual, Rich.
- **Papel do Agente**: Atuar como engenheiro de software no desenvolvimento de features, correções de bugs, refatoração e testes, garantindo qualidade, estabilidade e conformidade com a arquitetura do projeto.

---

## 2. Princípios de Engenharia e Padrões de Código
1. **Zero Comentários no Código:** É terminantemente proibido inserir comentários (`# ...`) em qualquer arquivo de código-fonte (`agy_tui/` ou `tests/`). Toda documentação, contexto e justificativas devem residir na Wiki local (`.wiki/`), na documentação formal ou nas mensagens de commit.
2. **Simplicidade e Modularidade:** Código limpo, desacoplado, modular e sem over-engineering. Nomes descritivos padronizados em inglês. Sem prints residuais, logs de depuração ad-hoc ou chamadas soltas.
3. **Qualidade e Linting:** Todo código deve aderir às regras configuradas no `pyproject.toml` e ser aprovado sem avisos ou erros pelo `ruff check .`.

---

## 3. Mapeamento da Base de Código
```text
frontal/
├── agy_tui/                  # Pacote principal da aplicação TUI
│   ├── app.py                # Aplicação Textual central (TUIApp), bindings globais, CSS e ciclo de eventos
│   ├── cli.py                # Ponto de entrada CLI (argument parsing, flags e inicialização)
│   ├── engine.py             # AgyEngine: orquestrador do subprocesso do motor (suporta AGY_BIN)
│   ├── protocol.py           # Parser tipado do protocolo JSON stream-json (StreamEvent)
│   ├── widgets.py            # Componentes visuais Textual (ChatInput, Sidebar, sub-barras, popups)
│   ├── session.py            # SessionManager: persistência de histórico em JSON (suporta AGY_SESSIONS_FILE)
│   ├── renderer.py           # MarkdownRenderer: renderização Rich e blocos de código
│   ├── clipboard.py          # ClipboardManager: suporte multiplataforma (OSC 52, utilitários do sistema)
│   └── diff.py               # DiffRegistry e visualizador de diffs estruturados
├── tests/                    # Suíte de testes automatizados
│   ├── conftest.py           # Fixture autouse de isolamento (mock engine e tmp_path para sessões)
│   ├── mock_engine.py        # Mock assíncrono do protocolo stream-json para testes isolados e rápidos
│   ├── test_app.py           # Testes unitários de widgets, comandos e layout
│   ├── test_fullscreen_tui.py# Testes de integração assíncronos da TUI completa
│   ├── test_renderer.py      # Testes de formatação e blocos de código
│   ├── test_protocol.py      # Testes de serialização e parsing de eventos JSON
│   ├── test_engine.py        # Testes de ciclo de vida do subprocesso
│   └── test_clipboard.py     # Testes do gerenciador de clipboard
├── .wiki/                    # Repositório da Wiki local (documentação técnica e ADRs)
├── .github/workflows/ci.yml  # Esteira de CI do GitHub Actions (ruff + pytest)
├── pyproject.toml            # Configuração do projeto, dependências e linter
├── README.md                 # Documentação pública principal
└── AGENTS.md                 # Diretrizes operacionais para agentes e colaboradores
```

---

## 4. Estratégia de Testes e Execução
1. **Ciclo Rápido durante Desenvolvimento (Unitários):**
   Executar apenas os testes focados (~1 segundo):
   ```bash
   pytest tests/test_app.py tests/test_renderer.py tests/test_protocol.py tests/test_engine.py
   ```
2. **Validação Final pré-commit (Integração e Suíte Completa):**
   ```bash
   pytest tests/test_fullscreen_tui.py
   pytest
   ```
3. **Isolamento Mandatório:**
   - O `tests/conftest.py` injeta automaticamente o `mock_engine.py` (via `AGY_BIN`) e isola os arquivos de sessão em `tmp_path` (via `AGY_SESSIONS_FILE`). Nunca inicializar o binário real de produção nem poluir arquivos locais de histórico durante a execução de testes.
4. **Proibição de Comandos Python Soltos:**
   - Não executar scripts Python ad-hoc via terminal (`python3 -c "..."`). Rodar estritamente testes (`pytest`), linters (`ruff`) ou comandos de controle de versão (`git`).

---

## 5. Fontes de Informação e Documentação
Para contexto aprofundado, regras de negócio e histórico técnico, consultar:
1. **Wiki Local do Projeto (`.wiki/`):**
   - `.wiki/Home.md`: Índice geral e status da documentação.
   - `.wiki/Conceito.md`: Visão do produto, princípios de design e ergonomia no terminal.
   - `.wiki/Arquitetura.md`: Topologia de camadas, ciclo de eventos `stream-json` e hierarquia Textual.
   - `.wiki/Decisoes.md`: Registro histórico de decisões de arquitetura (ADRs).
   - `.wiki/Operacao.md`: Comandos slash, atalhos de teclado e esteira de qualidade.
2. **Rastreamento de Demandas e GitHub:**
   - Repositório Git: `https://github.com/julioCoronetti/frontal`
   - Project Board: `https://github.com/users/julioCoronetti/projects/8` (Frontal)
   - Issues numeradas no GitHub para critérios de aceite detalhados.

---

## 6. Fluxo de Entrega e Governança Git
Para cada issue ou funcionalidade concluída:
1. Executar o linter: `ruff check .`
2. Validar a ausência total de comentários no código-fonte.
3. Executar a suíte completa de testes: `pytest`
4. Criar commits descritivos seguindo Conventional Commits (`feat(...)`, `fix(...)`, `docs(...)`, `test(...)`, `refactor(...)`).
5. Atualizar a documentação correspondente na Wiki (`.wiki/Decisoes.md`, `Arquitetura.md`, etc.) sempre que houver novas decisões técnicas ou mudanças arquiteturais.
