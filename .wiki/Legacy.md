# Protótipo Legado (Python / Textual)

## Contexto Histórico
A versão inicial do Frontal (`v0.1`) foi construída em Python utilizando as bibliotecas `Textual` e `Rich`, tendo como objetivo desacoplar uma interface TUI do motor `agy`.

O código integral desta versão foi preservado no repositório na branch Git dedicada `legacy`.

Para acessar o código legado:
```bash
git checkout legacy
```

## Componentes Desenvolvidos
- **Orquestração de Subprocesso (`agy_tui/engine.py`):** Gerenciamento assíncrono do ciclo de vida do motor `agy` com streams não-bloqueantes.
- **Parser de Protocolo (`agy_tui/protocol.py`):** Normalização e tipagem dos eventos JSON (`stream-json`).
- **Persistência de Sessões (`agy_tui/session.py`):** Histórico de conversas isolado por arquivos.
- **Área de Transferência Multiplataforma (`agy_tui/clipboard.py`):** Suporte ordenado a WSL2, ANSI OSC 52, Wayland e X11.
- **Suíte de Testes Automatizados (`tests/`):** Mock do motor e cobertura de testes funcionais.

## Razões da Descontinuação
1. **Gargalo no Streaming de Tokens:** Reflow contínuo da árvore DOM do Textual gerando lentidão e travamentos visuais.
2. **Ergonomia de Terminal Comprometida:** Captura agressiva de eventos de mouse e alt-screen forçado, impedindo a seleção nativa e a rolagem fluida do emulador de terminal.
3. **Decisão Arquitetural:** Transição para uma implementação focada em alta performance nativa, determinismo e simplicidade operacional.
