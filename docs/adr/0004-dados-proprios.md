# ADR 0004 — Base de dados própria, clean-room, modo genérico por padrão

**Status:** Proposta · **Data:** 2026-09-23

## Contexto

Um manager de futebol vive da base de dados. As opções realistas eram três:

1. **Extrair a base do jogo original** (ou de bases comunitárias derivadas dela).
2. **Licenciar** dados (FIFPro e associações nacionais).
3. **Construir a nossa** a partir de fontes abertas e geração procedural.

A opção 1 é cópia de obra protegida de terceiros — contamina o repositório inteiro,
inviabiliza distribuição e expõe todos os contribuidores. A opção 2 é economicamente
inacessível a um projeto sem receita.

Permanece ainda o problema de nomes reais: mesmo montando a base do zero, direitos de
personalidade (NIL) e marcas de clubes incidem sobre o **conteúdo**, não sobre o motor.

## Decisão

1. **Clean-room absoluto.** Nenhum arquivo, valor, tabela ou fórmula derivado do jogo
   original ou de bases de terceiros. Nenhum colaborador com acesso a material interno
   desses produtos contribui para o núcleo. Regra explícita no `CONTRIBUTING.md`.
2. **Base construída de fontes abertas** (openfootball, Wikidata) + **atributos gerados**
   por pipeline estatístico próprio, auditável, com curadoria da comunidade e proveniência
   por valor.
3. **Separação motor/conteúdo.** O jogo carrega *data packs*; o conteúdo não é parte do
   binário.
4. **Modo genérico é o padrão distribuído em lojas**: clubes e jogadores fictícios, com
   estrutura de competições real. Nomes factuais existem apenas em pack separado,
   instalado por escolha explícita do usuário e distribuído fora das lojas.

## Consequências

**Positivas** — risco jurídico drasticamente reduzido e **contido ao pack**, nunca ao
jogo; base melhora continuamente com a comunidade; dados publicados em CC0 beneficiam
outros projetos; o projeto não depende de nenhum terceiro para existir.

**Negativas** — a base inicial será pior que a de um produto licenciado; construir o
pipeline de ratings é trabalho real (estimado dentro do M1–M4); parte do público vai
reclamar de nomes fictícios no app da loja.

**Mitigação** — investir cedo na qualidade e na proveniência dos ratings; deixar a troca
para o pack da comunidade a **dois toques**; comunicar a razão com transparência.
