# Diretrizes e Padrões de Subagentes para o Repositório Frontal

Padrões de governança e delegação para subagentes no ciclo de desenvolvimento do Frontal, alinhados às evidências do SWE-bench, OpenHands e parecer do Argon Advisor.

## 1. Princípio da Assimetria de Contexto
A literatura empírica demonstra que subagentes LLM adicionam penalidade de 2x a 3x em consumo de tokens e latência devido à reinicialização de contexto.
Portanto, a delegação a subagentes só é justificada quando há **assimetria de contexto**: tarefas de alta intensidade de leitura ou raciocínio ruidoso cujo histórico intermediário deve ser descartado, retornando apenas a síntese conclusiva ao agente principal.

---

## 2. Perfis e Casos de Uso Recomendados

### Subagente `research` (Leitura e Investigação Isolada)
- **Quando usar**:
  - Auditoria profunda ou rastreamento de fluxos que atravessem 10 ou mais arquivos `.rs` (ex.: ciclo de vida de `EngineEvent`, transições de `TabState` ou invalidações do `RenderCache`).
  - Varredura de histórico extenso de transcrições ou documentação técnica.
- **Benefício**:
  - Toda a leitura bruta ocorre na sandbox isolada do subagente.
  - O agente principal recebe apenas a síntese estruturada, preservando a janela de contexto limpa e prevenindo *context rot*.
- **Restrição**:
  - Sandbox somente-leitura e sem acesso à rede.

### Subagente `self` (Execução de Diagnóstico Isolado)
- **Quando usar**:
  - Execução de experimentos ou hipóteses de troubleshooting que possam gerar múltiplas tentativas ou ruído de terminal antes de convergir na solução.
- **Benefício**:
  - Permite descartar o histórico de tentativas mal-sucedidas mantendo o agente principal no estado limpo.

---

## 3. Anti-Padrões Estritamente Proibidos

1. **Subagente para Tarefas Mecânicas Determinísticas**:
   - É proibido criar ou invocar subagentes LLM para rotinas como:
     - Execução da tríade Cargo (`fmt`, `clippy`, `test`).
     - Bump de SemVer no `Cargo.toml`.
     - Criação de tags Git anotadas.
     - Atualização de status no GitHub Project Board #8.
     - Sincronização do espelho `.wiki/`.
   - **Justificativa**: Essas tarefas são 100% determinísticas e devem ser executadas exclusivamente via **Skills locais com scripts** (`.agents/skills/`), com custo zero de inferência extra e sem risco de alucinação.

2. **Mutações Concorrentes no Mesmo Workspace**:
   - Subagentes não devem editar arquivos ou tentar commits concorrentes na mesma cópia de trabalho, evitando colisões de locks (`.git/index.lock` e `target/`).

3. **Operações de Rede sem Autorização**:
   - Subagentes não devem realizar chamadas de rede externas sem autorização explícita prévia do Senhor.
