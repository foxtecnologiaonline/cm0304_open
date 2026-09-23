# ADR 0002 — Determinismo é requisito; nenhum ponto flutuante no estado

**Status:** Proposta · **Data:** 2026-09-23

## Contexto

O jogo simula ~120 mil partidas por temporada em um mundo de 250 mil jogadores, em três
arquiteturas (x86-64, ARM64 e, potencialmente, WASM). Sem reprodutibilidade:

* não há como testar regressão de simulação (o resultado muda a cada execução);
* não há como calibrar balanceamento (o ruído esconde o efeito da mudança);
* não há reprodução confiável de bug reportado por jogador;
* o save cross-platform (RF-PL-02) fica impossível de garantir.

Ponto flutuante é a principal fonte de divergência entre plataformas: mesma expressão,
compiladores e unidades de vetorização diferentes, resultados diferentes no último bit —
e um único bit muda um gol, que muda uma temporada.

## Decisão

1. **Nenhum `f32`/`f64`** em cálculo que afete estado persistido. Matemática de simulação
   em inteiros ou ponto fixo `i32` (escala 1/1000). Float permitido apenas em render e
   interpolação visual.
2. **RNG por subsistema e entidade**, semeado por `hash(seed_mundo, domínio, id, tick)` —
   nunca um gerador global compartilhado.
3. **Iteração ordenada**: proibido iterar estrutura não-ordenada em lógica de jogo.
4. **Paralelismo apenas em partições independentes**, com redução em ordem fixa de id.
5. **Sem relógio do sistema, sem locale** dentro do núcleo.
6. **Verificação em CI**: hash de estado após N dias simulados, comparado entre as
   plataformas de build; divergência quebra o PR.
7. **Log de comandos** gravado junto ao save: reproduzir um bug é reexecutar o log.

## Consequências

**Positivas** — testes de regressão possíveis (golden masters); reprodução de bug em um
comando; base para replay de partida e, eventualmente, multiplayer lockstep; save
idêntico entre plataformas.

**Negativas** — ponto fixo é mais trabalhoso de escrever e revisar; paralelismo mais
restrito; certas bibliotecas numéricas ficam fora de alcance.

**Aceito conscientemente:** a perda de conveniência em troca de um sistema **testável**.
Em um projeto com 250 mil entidades e horizonte de 20 temporadas, não há alternativa.
