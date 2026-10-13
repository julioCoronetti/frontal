# Diretrizes do Agente - Repositório Frontal

## 1. Visão Geral do Projeto
- **Projeto**: Frontal.
- **Objetivo**: Interface de usuário em terminal (TUI) desacoplada, de alta performance e assíncrona para o Antigravity CLI (`agy`) e motores compatíveis com o protocolo `stream-json`.
- **Status do Projeto**: O projeto está em transição arquitetural. A pesquisa de engenharia reversa sobre os harnesses de referência (`agy` e `OpenCode`) está consolidada na Wiki, servindo de base para o planejamento da nova arquitetura.
- **Stack Oficial Ativa**: Rust (`ratatui`, `crossterm`, `tokio`, etc.).
- **Branches de Referência**:
  - `main`: Branch ativa para a nova arquitetura do harness em Rust.
  - `legacy` / `main-legacy`: Código integral e testes do protótipo inicial (Python / Textual).
- **Papel do Agente**: Atuar como engenheiro de software no desenvolvimento de features, correções de bugs, refatoração e testes, garantindo qualidade, estabilidade e conformidade com a arquitetura definida.

---

## 2. Princípios de Engenharia e Padrões de Código
1. **Zero Comentários no Código:** É terminantemente proibido inserir comentários no código-fonte. Toda documentação, contexto e justificativas devem residir na Wiki do GitHub, na documentação formal ou nas mensagens de commit.
2. **Simplicidade e Modularidade:** Código limpo, desacoplado, modular e sem over-engineering. Nomes descritivos padronizados em inglês. Sem prints residuais, logs de depuração ad-hoc ou chamadas soltas.
3. **Qualidade e Testes:** Todo código deve aderir às ferramentas de compilação, linting e testes da stack Rust (`cargo check`, `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo test`).

---

## 3. Fontes de Informação e Documentação
Para contexto aprofundado, regras de negócio e histórico técnico, consultar:
1. **Rastreamento de Demandas e GitHub:**
   - Repositório Git: `https://github.com/julioCoronetti/frontal`
   - Project Board: `https://github.com/users/julioCoronetti/projects/8` (Frontal)
   - Issues numeradas no GitHub para critérios de aceite detalhados.
2. **Documentação Oficial (Wiki do GitHub):**
   - Hub Central: `https://github.com/julioCoronetti/frontal/wiki` e `https://github.com/julioCoronetti/frontal/wiki/Home`
   - **Dossiê Antigravity CLI (`agy`):**
     - Visão Geral: `https://github.com/julioCoronetti/frontal/wiki/Agy`
     - Arquitetura & Protocolo: `https://github.com/julioCoronetti/frontal/wiki/Agy-Architecture`
     - Motor & Ferramentas: `https://github.com/julioCoronetti/frontal/wiki/Agy-Backend`
     - Interface & TUI: `https://github.com/julioCoronetti/frontal/wiki/Agy-UI`
   - **Dossiê OpenCode:**
     - Visão Geral: `https://github.com/julioCoronetti/frontal/wiki/OpenCode`
     - Arquitetura & Monorepo: `https://github.com/julioCoronetti/frontal/wiki/OpenCode-Architecture`
     - Motor & Streaming: `https://github.com/julioCoronetti/frontal/wiki/OpenCode-Backend`
     - Interface & TUI: `https://github.com/julioCoronetti/frontal/wiki/OpenCode-UI`
   - **Planejamento e Roadmap (`Board-Roadmap`):**
     - Página: `https://github.com/julioCoronetti/frontal/wiki/Board-Roadmap`
     - *Diretriz de Uso:* O conteúdo do `Board-Roadmap` representa uma proposta conceitual e rascunho de ideias derivado da pesquisa técnica. Não deve ser tratado como regra rígida ou imutável; o planejamento formal das fases e regras de negócio está em construção pelo mantenedor com base na pesquisa da Wiki.
   - **Histórico do Legado:**
     - Página: `https://github.com/julioCoronetti/frontal/wiki/Legacy`

---

## 4. Fluxo de Entrega e Governança Git
Para cada issue ou funcionalidade concluída:
1. Executar ferramentas de validação e linters configurados (`cargo check`, `cargo clippy`, `cargo fmt --check`).
2. Validar a ausência total de comentários no código-fonte.
3. Executar a suíte de testes automatizados (`cargo test`).
4. Criar commits descritivos seguindo Conventional Commits (`feat(...)`, `fix(...)`, `docs(...)`, `test(...)`, `refactor(...)`, `chore(...)`).

---

## 5. Ciclo de Vida do Board e Governança de Tarefas (Workflow de Cards)
O ciclo de vida dos cards no Project Board (`https://github.com/users/julioCoronetti/projects/8`) segue uma esteira rigorosa em quatro estados:

1. **Seleção e Início (`Todo` -> `In Progress`):**
   - Retirar o card prioritário do Backlog (`Todo`) e movê-lo para `In Progress`.
   - Implementar estritamente o escopo demandado, sem comentários no código e com nomes descritivos.

2. **Verificação Preliminar e Transição para Revisão (`In Progress` -> `In Review`):**
   - Executar validações de código e testes.
   - Concluída a implementação e verificada sua funcionalidade inicial, mover o card para `In Review`.
   - **Nunca mover diretamente para `Done`**.

3. **Ciclo de Validação e Testes com Perspectiva Alternativa (`In Review`):**
   - O usuário analisa e valida o comportamento da funcionalidade.
   - O agente deve reexecutar testes adotando intencionalmente **perspectivas diferentes** (testes adversariais, casos de borda, estresse de entrada, comportamentos inesperados fora do escopo e busca ativa de falhas na entrega).
   - Havendo qualquer inconsistência, bug ou necessidade de ajuste:
     - Realizar a correção necessária.
     - O card **permanece em `In Review`**.
     - Repetir a bateria de testes sob novas perspectivas até que nenhuma nova alteração seja requerida.

4. **Conclusão da Revisão e Transição para `Done`:**
   - Quando todos os testes passarem sem necessidade de novas alterações, o agente emite um aviso simples e direto:
     > *"Revisão completa. Nenhuma alteração pendente."*
   - O card só pode ser movido para `Done` após a validação e autorização explícita do usuário.
