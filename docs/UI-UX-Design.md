# Diretrizes de UI/UX (Padrão OpenCode)

## 1. Filosofia Visual
O Frontal adota a estética minimalista e ergonômica inspirada no **OpenCode**:
- **Densidade Visual Otimizada:** Sem cabeçalhos decorativos volumosos que desperdicem linhas verticais do terminal.
- **Hierarquia de Mensagens Clara:**
  - Mensagens do usuário possuem barra lateral discreta em azul (`#89b4fa`) e fundo escuro suave (`#1e1e2e`), sem rótulos redundantes como `USER ❯`.
  - Mensagens do assistente fluem diretamente com tipografia legível e renderização Rich de sintaxe.
  - Avisos de sistema utilizam formato em linha única sem caixas de borda pesadas (`● mensagem`).
- **Raciocínio Dinâmico (*Thoughts*):**
  - Indicador ativo em rotação durante geração (`⠋ Thinking...`).
  - Colapso automático pós-geração em linha única compacta (`▣ Thought`), expansível sob demanda via clique ou `/thoughts`.
- **Linha de Status do Turno:**
  - Exibição limpa em linha única no rodapé da mensagem: `{model_name} · {duration}s ({tok/s} tok/s)`.
  - Sem caixas decorativas ou marcadores visuais supérfluos.

---

## 2. Área de Transferência e Seleção
- **`Ctrl+C` Contextual:**
  - Quando um turno está em execução: Interrompe a execução do agente de forma segura.
  - Quando a aplicação está ociosa:
    1. Se houver texto selecionado no prompt de entrada: Copia a seleção para a área de transferência do sistema.
    2. Se não houver seleção: Copia a última resposta do assistente para a área de transferência.
    3. Se houver texto no input: Copia o conteúdo do prompt.
- **Clique para Copiar:**
  - Clicar em qualquer `MessageCard` (pergunta do usuário) ou botão de bloco de código (`📋 Copy`) copia imediatamente o conteúdo para a área de transferência.
- **Comandos Slash de Cópia:**
  - `/copy` ou `/c`: Copia a última resposta do assistente (quando sem argumentos) ou o bloco de código específico (ex: `/copy 1`).
  - `/copyall` ou `/ca`: Copia a resposta integral do turno.
- **Seleção Nativa no Terminal:**
  - Emuladores de terminal que capturam eventos do mouse permitem seleção nativa do buffer ao segurar `Shift` durante o arrasto do ponteiro (`Shift + Seleção`), copiando via atalho padrão do terminal (`Ctrl+Shift+C`).
- **Seleção no Prompt:**
  - `Ctrl+A`: Seleciona todo o conteúdo do buffer de entrada no `ChatInput`.

---

## 3. Modais Centralizados
- Os diálogos (`/model`, `/diff`, `/resume`) utilizam telas modais flutuantes (`ModalScreen`) com fundo semitransparente (`rgba(0,0,0,0.6)`), mantendo o contexto da conversa visível ao fundo.
- O diálogo `/model` implementa um fluxo em duas etapas:
  1. Etapa 1: Seleção do modelo base (ex: `gemini-3.7-flash`, `claude-sonnet-5-5`).
  2. Etapa 2: Seleção do nível de raciocínio / esforço (`high`, `medium`, `low`).
  3. Navegação reversível com `Esc` (retorna ao passo anterior ou fecha o modal).
