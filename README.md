# cm0304_open — reimplementação aberta de um manager de futebol clássico

> **Codinome do repositório:** `cm0304_open`
> **Nome público sugerido:** *Touchline* (alternativas em [`docs/05-dados-e-legal.md`](docs/05-dados-e-legal.md))
> **Status:** 📐 fase de escopo — nenhuma linha de código de produção escrita ainda.

Projeto para recriar, de forma **aberta (open source)** e **legalmente limpa**, a
experiência de jogo do *Championship Manager 03/04* — profundidade de base de dados,
ritmo de jogo, visão 2D top-down da partida, atributos 1–20 — para **Windows 10/11**,
**Android** e **iOS**, com o mesmo save rodando em todas as plataformas.

Este repositório contém, neste momento, **apenas o escopo de engenharia**. Ele existe
para que a decisão de construir (ou não) seja tomada com números, riscos e trade-offs
explícitos, e não com entusiasmo.

## Leia nesta ordem

| # | Documento | O que responde |
|---|-----------|----------------|
| 00 | [Escopo mestre](docs/00-escopo.md) | O que é, o que **não** é, MVP, fases, critérios de sucesso |
| 01 | [Requisitos](docs/01-requisitos.md) | Épicos, features, requisitos não-funcionais, backlog priorizado |
| 02 | [Arquitetura](docs/02-arquitetura.md) | Stack, camadas, determinismo, performance, build multiplataforma |
| 03 | [Modelo de dados](docs/03-modelo-de-dados.md) | Domínio, schema, atributos, CA/PA, saves |
| 04 | [Motor de partida](docs/04-motor-de-partida.md) | Simulação híbrida, calibração, orçamento de CPU |
| 05 | [Dados e jurídico](docs/05-dados-e-legal.md) | Fontes abertas, licenças, direitos de imagem, lojas |
| 06 | [UI/UX](docs/06-ui-ux.md) | Navegação desktop e mobile, densidade, acessibilidade |
| 07 | [Roadmap](docs/07-roadmap.md) | Marcos M0–M5, estimativas, equipe, custo |
| 08 | [Qualidade](docs/08-qualidade-e-testes.md) | Testes, golden masters, balanceamento, CI/CD |
| 09 | [Riscos](docs/09-riscos.md) | Registro de riscos com mitigação e gatilhos |
| — | [ADRs](docs/adr/) | Decisões arquiteturais registradas |

## Resumo executivo em 10 linhas

* **Não é** um port, um patch, um "reskin" nem um projeto que lê arquivos do jogo original.
  É uma **reimplementação limpa** (clean-room): nenhum byte, asset ou base de dados do
  produto de 2003 entra aqui. Veja [`docs/05`](docs/05-dados-e-legal.md).
* **Núcleo** de simulação em **Rust** — determinístico, sem GC, compila para Windows,
  Android, iOS e WASM. **UI** em **Flutter** — um código para desktop e mobile, com
  listas densas virtualizadas e a visão 2D do campo em canvas próprio.
* **Determinismo é requisito de produto**, não detalhe: mesma seed + mesmos comandos =
  mesmo campeonato, byte a byte. É o que torna testável um sistema com 250 mil jogadores.
* **Dados** vêm de fontes abertas (openfootball/Wikidata, domínio público/CC0) e de
  geração procedural, com *data packs* da comunidade carregados pelo usuário.
* **Licença recomendada:** MPL-2.0 no código (compatível com App Store, ao contrário da
  GPL) e CC0/ODbL nos dados. Justificativa em [`docs/adr/0003`](docs/adr/0003-licenciamento.md).
* **Esforço estimado:** 14–18 meses até 1.0 com um time de 3–5 pessoas; ~30 meses em
  regime de hobby. O detalhamento por marco está em [`docs/07`](docs/07-roadmap.md).
* **Maior risco não-técnico:** direitos de personalidade e marcas sobre nomes reais de
  jogadores e clubes. Maior risco técnico: **balanceamento** do motor de partida — não a
  sua implementação.

## Licença

Ainda não definida formalmente (ver ADR 0003). A recomendação vigente é **MPL-2.0** para
código e **CC0-1.0 / ODbL** para dados. Nenhum arquivo deste repositório deriva de
software ou base de dados de terceiros protegidos.

*Championship Manager* é marca do seu detentor atual (Embracer Group, via aquisição da
Eidos em 2022). Este projeto **não é afiliado, endossado ou licenciado** por ele, não
utiliza sua marca no produto final e não distribui nenhum de seus conteúdos.
