# Governança de Tarefas e Roadmap do Board

## 1. Ciclo de Vida do Project Board (Projeto 8)
A esteira de desenvolvimento obedece ao fluxo estrito estipulado no `AGENTS.md`:

```mermaid
stateDiagram-v2
    [*] --> Todo: Backlog
    Todo --> InProgress: Início da Tarefa
    InProgress --> InReview: Validação Preliminar
    InReview --> InReview: Ciclo Adversarial de Testes
    InReview --> Done: Autorização Explícita do Usuário
    Done --> [*]
```

- **Todo -> In Progress:** Seleção e início do desenvolvimento.
- **In Progress -> In Review:** Conclusão da implementação, aprovação no linter (`ruff check .`) e suíte de testes (`pytest`).
- **In Review:** Análise do usuário e testes contínuos sob perspectivas alternativas (casos de borda, estresse, comportamento adverso).
- **In Review -> Done:** Exclusivamente após autorização e validação formal do Senhor.

---

## 2. Inventário de Cards e Status no Board

| Issue | Tipo | Título | Status Atual | Escopo & Ajustes de Ambiente |
| :---: | :---: | :--- | :---: | :--- |
| **#1** | `feat(ui)` | Tela inicial de boas-vindas centralizada (Welcome State) | `Todo` | Exibir logo e prompt centralizados na abertura; transicionar para 2 colunas ao primeiro prompt (padrão OpenCode). |
| **#2** | `feat(ui)` | Refatoração visual do fluxo de mensagens do chat | `Todo` | Contêineres de mensagens sem bordas pesadas, barra lateral azul, thoughts colapsáveis e rodapé de turno minimalista. (Maior parte implementada; pendente transição formal). |
| **#3** | `feat(ui)` | Redesenho da barra de entrada com sub-barra interna | `Done` | Sub-barra com modelo, modo e esforço; rodapé com tokens e path. |
| **#4** | `feat(ui)` | Barra lateral minimalista focada em contexto | `Done` | Sidebar retrátil (`Ctrl+B`) focada em janela de contexto e telemetria essencial. |
| **#5** | `feat(ui)` | Menu de autocomplete de comandos expandido (Slash Pop-up) | `Todo` | Pop-up com duas colunas alinhadas (comando + descrição) com seleção via teclado e Tab. |
| **#6** | `feat(core)` | Sistema de abas superiores para múltiplas sessões em paralelo | `Todo` | Gerenciamento de abas (`Ctrl+T`), isolamento de processos e alternância rápida. |
| **#7** | `feat(ui)` | Modais de diálogo flutuantes para ações (/model, /diff, /resume) | `In Review` | Modais flutuantes centralizados sobrepostos à viewport com fundo translúcido. |
| **#8** | `feat(core)` | Seletor de modelo versátil em duas etapas (Model + Effort) | `In Review` | Diálogo `/model` e atalho `Ctrl+M` guiando seleção de modelo e esforço de raciocínio. |
| **#9** | `fix(telemetry)` | Correção do cálculo de tempo de resposta por turno | `Done` | Medição de latência real por turno do envio ao evento de resultado. |
| **#10** | `feat(telemetry)` | Métrica de velocidade de geração em tokens por segundo (tok/s) | `Done` | Cálculo do throughput de tokens por segundo exibido na finalização do turno. |
| **#11** | `feat(ui)` | Refatoração ergonômica do ChatInput e rodapé no padrão OpenCode | `Todo` | Alinhamento compacto sem bordas duplicadas e integração fluida entre input e status. |

---

## 3. Próximos Passos Prioritários
1. **Concluir Revisão dos Cards #7 e #8:** Aguardar autorização do Senhor para transição para `Done`.
2. **Card #2:** Mover para `In Progress` / `In Review` para formalizar as melhorias do fluxo de mensagens já incorporadas na base.
3. **Card #1 e #11:** Implementação do Welcome State centralizado e polimento ergonômico complementar da caixa de entrada.
