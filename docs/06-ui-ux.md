# 06 — UI / UX

> O desafio de UX não é "fazer bonito". É caber **a mesma densidade de informação** de um
> monitor de 27" na tela de um celular, sem transformar o jogo em outra coisa.

---

## 1. Princípios

| # | Princípio | Consequência prática |
|---|---|---|
| 1 | **Continuar é o verbo principal** | Um botão sempre visível e sempre no mesmo lugar (canto inferior direito no desktop; FAB persistente no mobile). Atalho `Espaço`/`Enter` no desktop. |
| 2 | **Densidade é feature, não defeito** | Listas mostram 15–25 linhas úteis no desktop. Nada de cartões gigantes com 3 informações. |
| 3 | **Três cliques para qualquer lugar** | Navegação lateral no desktop, barra inferior + busca global no mobile. |
| 4 | **Nada de tela de carregamento** | Processamento roda em background com progresso; o usuário continua navegando. |
| 5 | **Informação oculta permanece oculta** | A névoa de guerra é respeitada até no *tooltip* (ver [`03`](03-modelo-de-dados.md#6-conhecimento-névoa-de-guerra)). |
| 6 | **Texto é a interface** | Sem animação decorativa, sem som obrigatório, sem transição que atrase leitura. |

---

## 2. Mapa de navegação

```
┌ Início ─────────── caixa de entrada, próximo jogo, resumo do clube
├ Elenco ─────────── lista, perfil do jogador, contratos, treino
├ Tática ─────────── escalação, formação, instruções, planos alternativos
├ Partida ────────── pré-jogo → ao vivo (2D/texto) → relatório
├ Mercado ────────── busca, shortlist, ofertas, empréstimos, olheiros
├ Competições ────── tabela, artilharia, calendário, resultados do mundo
├ Clube ──────────── finanças, estádio, base, comissão técnica, diretoria
└ Meta ───────────── save, opções, packs, editor (desktop), estatísticas
```

Mesma árvore nas duas plataformas — muda a **projeção**, não a estrutura. Um jogador que
aprendeu no PC não se perde no celular.

---

## 3. Desktop (Windows 10/11)

```
┌───────────────────────────────────────────────────────────────────────┐
│ [logo]  Meu Clube ▾   Temporada 2026/27   │ 12 Abr  │  🔔 3  │ ⚙     │  ← barra global
├──────────┬────────────────────────────────────────────────────────────┤
│ Início   │  ELENCO                        [filtros ▾] [colunas ▾] [⌕] │
│ Elenco ◄ │ ┌──────────────────────────────────────────────────────┐  │
│ Tática   │ │ Nome            Pos  Ida  Cond Mor  Val    Contrato  │  │
│ Partida  │ │ ● J. Andrade    MC   21   98%  ▲    2,4M   2029      │  │
│ Mercado  │ │ ○ R. Kowalski   ZC   29   87%  ▬    1,1M   2027      │  │
│ Compet.  │ │ ...                            (linhas densas, 24px)  │  │
│ Clube    │ └──────────────────────────────────────────────────────┘  │
│ Meta     │  ── painel de detalhe (opcional, lado a lado) ──          │
├──────────┴────────────────────────────────────────────────────────────┤
│  Próximo: Sábado — Fora vs. Atlético (Liga)          [ CONTINUAR ▶ ]  │
└───────────────────────────────────────────────────────────────────────┘
```

* **Tabelas de verdade**: ordenação multi-coluna, colunas configuráveis e persistidas,
  seleção múltipla, comparação lado a lado de até 4 jogadores.
* **Teclado completo** (RF-PL-05): `Espaço` continuar, `/` busca global, `1–8` seções,
  `J/K` navegar linhas, `Enter` abrir, `Esc` voltar, `Ctrl+S` salvar.
* Janela redimensionável de 1024×720 a 4K, com DPI correto e 3 densidades
  (compacta/normal/confortável).

---

## 4. Mobile (Android / iOS)

O erro clássico é portar a tabela do desktop e obrigar a rolagem horizontal. A solução é
**projeção adaptativa**: a mesma consulta rende uma linha de tabela no desktop e uma
linha compacta de 3 zonas no celular.

```
┌─────────────────────────┐
│ ⌂ Meu Clube    12 Abr 🔔│
├─────────────────────────┤
│ ELENCO      [⌕] [filtro]│
│ ┌─────────────────────┐ │
│ │ J. Andrade      MC  │ │   ← zona 1: identidade
│ │ 21a · 98% · ▲       │ │   ← zona 2: estado (métricas escolhidas pelo usuário)
│ │ 2,4M · até 2029     │ │   ← zona 3: contrato/valor
│ ├─────────────────────┤ │
│ │ R. Kowalski     ZC  │ │
│ └─────────────────────┘ │
│                    ( ▶ )│   ← continuar, alcance do polegar
├─────────────────────────┤
│ ⌂    👥    ⚽    💱    ☰ │   ← barra inferior: 5 destinos
└─────────────────────────┘
```

Decisões específicas de mobile:

| Tema | Decisão |
|---|---|
| **Alcance do polegar** | Ações primárias no terço inferior. Nada crítico no topo. |
| **Gestos** | Deslizar na linha: ações rápidas (shortlist, comparar, oferta). Puxar para atualizar: nunca (não há rede). |
| **Tabela quando necessária** | Visão "planilha" opcional em paisagem, com fixação da coluna de nome. |
| **Tablet / desktop dobrável** | ≥ 600dp ativa layout de duas colunas (lista + detalhe), igual ao desktop. |
| **Retomada instantânea** | O app volta exatamente na tela e posição de rolagem anteriores; autosave a cada dia simulado. |
| **Interrupção** | Processamento continua em *foreground service* curto no Android; se for morto, retoma do último dia consistente. |
| **Bateria e calor** | Simulação em lote limitada a N threads e com *throttle* térmico; alvo RNF-11. |

---

## 5. A partida

Três modos, mesma tela, alternáveis **durante** o jogo (o resultado não muda — ver
[`04`](04-motor-de-partida.md#3-os-três-regimes)):

| Modo | Desktop | Mobile |
|---|---|---|
| **2D top-down** | campo central, comentário lateral, banco e estatísticas em abas | campo no topo (16:9), comentário rolando abaixo, controles em folha deslizante |
| **Texto comentado** | lista de eventos com destaque para gols/cartões | idem, com tipografia maior |
| **Resultado instantâneo** | pula para o relatório | idem |

* Velocidade em 4 níveis + pausa; pausa automática configurável (gol, lesão, vermelho).
* Substituição em 2 toques no mobile (arrastar não é confiável em tela pequena).
* O campo 2D é desenhado em canvas próprio com interpolação entre ticks — 60 fps no
  desktop, 30 fps garantidos no mobile (RNF-05).

---

## 6. Design system

* **Tokens** (cor, espaçamento, tipografia, densidade) definidos uma vez e compartilhados
  pelas plataformas; tema claro e escuro, ambos de primeira classe.
* **Tipografia**: fonte com boa leitura de números tabulares (dígitos de largura fixa é
  requisito — tabelas com números desalinhados são ilegíveis).
* **Cor com significado limitado**: verde/vermelho apenas para variação; nunca como única
  portadora de informação (acessibilidade).
* **Componentes de dados** próprios: `DataTable` virtualizada, `AttributeGrid` (com
  suporte a faixas da névoa de guerra), `FormationPitch`, `TimelineBar`, `MoneyText`.

---

## 7. Acessibilidade (RNF-07 / RF-PL-07)

- Escala de fonte de 85% a 150% sem quebrar layout.
- Contraste mínimo AA (4.5:1) em texto, 3:1 em elementos de interface.
- Alvos de toque ≥ 44×44 pt no mobile.
- Leitor de tela nas listas principais e na caixa de entrada; ordem de foco previsível.
- Modo daltônico (paleta alternativa para variações e para as cores de time no 2D).
- Nenhuma informação transmitida só por cor, só por som ou só por animação.

---

## 8. Localização (RF-PL-06)

* PT-BR, EN e ES no 1.0; arquitetura sem string fixa em código desde o M2.
* **Comentário de partida é gerado a partir de eventos tipados**, com templates por
  idioma — isso evita a armadilha de traduzir frases concatenadas.
* Pluralização e gênero via ICU MessageFormat.
* Formatos de data, número e moeda por locale, mas **datas do jogo** sempre vindas do
  calendário do núcleo (nunca do relógio do sistema).

---

## 9. Primeira sessão

Sem tutorial obrigatório — o público-alvo o odiaria. Em vez disso:

1. **Início rápido**: escolher país → clube → nome do técnico → jogar (≤ 60 s).
2. Dicas contextuais dispensáveis em definitivo com um toque.
3. "Modo guiado" opcional: sugere escalação, treino e alvos de mercado (usa a mesma IA
   dos outros clubes — e pode ser desligado a qualquer momento).
4. Seleção de profundidade: quantos países carregar (afeta diretamente RAM e velocidade,
   com estimativa mostrada **antes** de começar, inclusive no mobile).
